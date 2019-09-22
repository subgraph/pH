// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "sommelier.h"

#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <wayland-client.h>

#include "aura-shell-client-protocol.h"

#define MAX_OUTPUT_SCALE 2

#define INCH_IN_MM 25.4

// Legacy X11 applications use DPI to decide on their scale. This value is what
// the convention for a "normal" scale is. One way to verify the convention is
// to note the DPI of a typical monitor circa ~2005, i.e. 20" 1080p.
#define DEFACTO_DPI 96

double sl_output_aura_scale_factor_to_double(int scale_factor) {
  // Aura scale factor is an enum that for all currently know values
  // is a scale value multipled by 1000. For example, enum value for
  // 1.25 scale factor is 1250.
  return scale_factor / 1000.0;
}

int dpi_to_physical_mm(double dpi, int px) {
  return px * (INCH_IN_MM / dpi);
}

void sl_output_get_host_output_state(struct sl_host_output* host,
                                     int* scale,
                                     int* physical_width,
                                     int* physical_height,
                                     int* width,
                                     int* height) {
  // The user's chosen zoom level.
  double current_scale =
      sl_output_aura_scale_factor_to_double(host->current_scale);

  // The scale applied to a screen at the default zoom. I.e. this value
  // determines the meaning of "100%" zoom, and how zoom relates to the
  // apparent resolution:
  //
  //    apparent_res = native_res / device_scale_factor * current_scale
  //
  // e.g.: On a device with a DSF of 2.0, 80% zoom really means "apply 1.6x
  // scale", and 50% zoom would give you an apparent resolution equal to the
  // native one.
  double device_scale_factor =
      sl_output_aura_scale_factor_to_double(host->device_scale_factor);

  // Optimistically, we will try to apply the scale that the user chose.
  // Failing that, we will use the scale set for this wl_output.
  double applied_scale = device_scale_factor * current_scale;
  if (!host->ctx->aura_shell) {
    applied_scale = host->scale_factor;
  }

  int target_dpi = DEFACTO_DPI;
  if (host->ctx->xwayland) {
    // For X11, we must fix the scale to be 1 (since X apps typically can't
    // handle scaling). As a result, we adjust the resolution (based on the
    // scale we want to apply and sommelier's configuration) and the physical
    // dimensions (based on what DPI we want the applications to use). E.g.:
    //  - Device scale is 1.25x, with 1920x1080 resolution on a 295mm by 165mm
    //    screen.
    //  - User chosen zoom is 130%
    //  - Sommelier is scaled to 0.5 (a.k.a low density). Since ctx->scale also
    //    has the device scale, it will be 0.625 (i.e. 0.5 * 1.25).
    //  - We want the DPI to be 120 (i.e. 96 * 1.25)
    //     - Meaning 0.21 mm/px
    //  - We report resolution 738x415 (1920x1080 * 0.5 / 1.3)
    //  - We report dimensions 155mm by 87mm (738x415 * 0.21)
    // This is mostly expected, another way of thinking about them is that zoom
    // and scale modify the application's understanding of length:
    //  - Increasing the zoom makes lengths appear longer (i.e. fewer mm to work
    //    with over the same real length).
    //  - Scaling the screen does the inverse.
    if (scale)
      *scale = 1;
    *width = host->width * host->ctx->scale / applied_scale;
    *height = host->height * host->ctx->scale / applied_scale;

    target_dpi = DEFACTO_DPI * device_scale_factor;
    *physical_width = dpi_to_physical_mm(target_dpi, *width);
    *physical_height = dpi_to_physical_mm(target_dpi, *height);
  } else {
    // For wayland, we directly apply the scale which combines the user's chosen
    // preference (from aura) and the scale which this sommelier was configured
    // for (i.e. based on ctx->scale, which comes from the env/cmd line).
    //
    // See above comment: ctx->scale already has the device_scale_factor in it,
    // so this maths actually looks like:
    //
    //              applied / ctx->scale
    //      = (current*DSF) / (config*DSF)
    //      =       current / config
    //
    // E.g. if we configured sommelier to scale everything 0.5x, and the user
    // has chosen 130% zoom, we are applying 2.6x scale factor.
    int s = MIN(ceil(applied_scale / host->ctx->scale), MAX_OUTPUT_SCALE);

    if (scale)
      *scale = s;
    *physical_width = host->physical_width;
    *physical_height = host->physical_height;
    *width = host->width * host->ctx->scale * s / applied_scale;
    *height = host->height * host->ctx->scale * s / applied_scale;
    target_dpi = (*width * INCH_IN_MM) / *physical_width;
  }

  if (host->ctx->dpi.size) {
    int adjusted_dpi = *((int*)host->ctx->dpi.data);
    int* p;

    // Choose the DPI bucket which is closest to the target DPI which we
    // calculated above.
    wl_array_for_each(p, &host->ctx->dpi) {
      if (abs(*p - target_dpi) < abs(adjusted_dpi - target_dpi))
        adjusted_dpi = *p;
    }

    *physical_width = dpi_to_physical_mm(adjusted_dpi, *width);
    *physical_height = dpi_to_physical_mm(adjusted_dpi, *height);
  }
}

void sl_output_send_host_output_state(struct sl_host_output* host) {
  int scale;
  int physical_width;
  int physical_height;
  int width;
  int height;

  sl_output_get_host_output_state(host, &scale, &physical_width,
                                  &physical_height, &width, &height);

  // Use density of internal display for all Xwayland outputs. X11 clients
  // typically lack support for dynamically changing density so it's
  // preferred to always use the density of the internal display.
  if (host->ctx->xwayland) {
    struct sl_host_output* output;

    wl_list_for_each(output, &host->ctx->host_outputs, link) {
      if (output->internal) {
        int internal_width;
        int internal_height;

        sl_output_get_host_output_state(output, NULL, &physical_width,
                                        &physical_height, &internal_width,
                                        &internal_height);

        physical_width = (physical_width * width) / internal_width;
        physical_height = (physical_height * height) / internal_height;
        break;
      }
    }
  }

  // X/Y are best left at origin as managed X windows are kept centered on
  // the root window. The result is that all outputs are overlapping and
  // pointer events can always be dispatched to the visible region of the
  // window.
  wl_output_send_geometry(host->resource, 0, 0, physical_width, physical_height,
                          host->subpixel, host->make, host->model,
                          host->transform);
  wl_output_send_mode(host->resource, host->flags | WL_OUTPUT_MODE_CURRENT,
                      width, height, host->refresh);
  if (wl_resource_get_version(host->resource) >= WL_OUTPUT_SCALE_SINCE_VERSION)
    wl_output_send_scale(host->resource, scale);
  if (wl_resource_get_version(host->resource) >= WL_OUTPUT_DONE_SINCE_VERSION)
    wl_output_send_done(host->resource);
}

static void sl_output_geometry(void* data,
                               struct wl_output* output,
                               int x,
                               int y,
                               int physical_width,
                               int physical_height,
                               int subpixel,
                               const char* make,
                               const char* model,
                               int transform) {
  struct sl_host_output* host = wl_output_get_user_data(output);

  host->x = x;
  host->y = y;
  host->physical_width = physical_width;
  host->physical_height = physical_height;
  host->subpixel = subpixel;
  free(host->model);
  host->model = strdup(model);
  free(host->make);
  host->make = strdup(make);
  host->transform = transform;
}

static void sl_output_mode(void* data,
                           struct wl_output* output,
                           uint32_t flags,
                           int width,
                           int height,
                           int refresh) {
  struct sl_host_output* host = wl_output_get_user_data(output);

  host->flags = flags;
  host->width = width;
  host->height = height;
  host->refresh = refresh;
}

static void sl_output_done(void* data, struct wl_output* output) {
  struct sl_host_output* host = wl_output_get_user_data(output);

  // Early out if scale is expected but not yet know.
  if (host->expecting_scale)
    return;

  sl_output_send_host_output_state(host);

  // Expect scale if aura output exists.
  if (host->aura_output)
    host->expecting_scale = 1;
}

static void sl_output_scale(void* data,
                            struct wl_output* output,
                            int32_t scale_factor) {
  struct sl_host_output* host = wl_output_get_user_data(output);

  host->scale_factor = scale_factor;
}

static const struct wl_output_listener sl_output_listener = {
    sl_output_geometry, sl_output_mode, sl_output_done, sl_output_scale};

static void sl_aura_output_scale(void* data,
                                 struct zaura_output* output,
                                 uint32_t flags,
                                 uint32_t scale) {
  struct sl_host_output* host = zaura_output_get_user_data(output);

  if (flags & ZAURA_OUTPUT_SCALE_PROPERTY_CURRENT)
    host->current_scale = scale;
  if (flags & ZAURA_OUTPUT_SCALE_PROPERTY_PREFERRED)
    host->preferred_scale = scale;

  host->expecting_scale = 0;
}

static void sl_aura_output_connection(void* data,
                                      struct zaura_output* output,
                                      uint32_t connection) {
  struct sl_host_output* host = zaura_output_get_user_data(output);

  host->internal = connection == ZAURA_OUTPUT_CONNECTION_TYPE_INTERNAL;
}

static void sl_aura_output_device_scale_factor(void* data,
                                               struct zaura_output* output,
                                               uint32_t device_scale_factor) {
  struct sl_host_output* host = zaura_output_get_user_data(output);

  host->device_scale_factor = device_scale_factor;
}

static const struct zaura_output_listener sl_aura_output_listener = {
    sl_aura_output_scale, sl_aura_output_connection,
    sl_aura_output_device_scale_factor};

static void sl_destroy_host_output(struct wl_resource* resource) {
  struct sl_host_output* host = wl_resource_get_user_data(resource);

  if (host->aura_output)
    zaura_output_destroy(host->aura_output);
  if (wl_output_get_version(host->proxy) >= WL_OUTPUT_RELEASE_SINCE_VERSION) {
    wl_output_release(host->proxy);
  } else {
    wl_output_destroy(host->proxy);
  }
  wl_resource_set_user_data(resource, NULL);
  wl_list_remove(&host->link);
  free(host->make);
  free(host->model);
  free(host);
}

static void sl_bind_host_output(struct wl_client* client,
                                void* data,
                                uint32_t version,
                                uint32_t id) {
  struct sl_output* output = (struct sl_output*)data;
  struct sl_context* ctx = output->ctx;
  struct sl_host_output* host;

  host = malloc(sizeof(*host));
  assert(host);
  host->ctx = ctx;
  host->resource = wl_resource_create(client, &wl_output_interface,
                                      MIN(version, output->version), id);
  wl_resource_set_implementation(host->resource, NULL, host,
                                 sl_destroy_host_output);
  host->proxy = wl_registry_bind(wl_display_get_registry(ctx->display),
                                 output->id, &wl_output_interface,
                                 wl_resource_get_version(host->resource));
  wl_output_set_user_data(host->proxy, host);
  wl_output_add_listener(host->proxy, &sl_output_listener, host);
  host->aura_output = NULL;
  // We assume that first output is internal by default.
  host->internal = wl_list_empty(&ctx->host_outputs);
  host->x = 0;
  host->y = 0;
  host->physical_width = 0;
  host->physical_height = 0;
  host->subpixel = WL_OUTPUT_SUBPIXEL_UNKNOWN;
  host->make = strdup("unknown");
  host->model = strdup("unknown");
  host->transform = WL_OUTPUT_TRANSFORM_NORMAL;
  host->flags = 0;
  host->width = 1024;
  host->height = 768;
  host->refresh = 60000;
  host->scale_factor = 1;
  host->current_scale = 1000;
  host->preferred_scale = 1000;
  host->device_scale_factor = 1000;
  host->expecting_scale = 0;
  wl_list_insert(ctx->host_outputs.prev, &host->link);
  if (ctx->aura_shell) {
    host->expecting_scale = 1;
    host->internal = 0;
    host->aura_output =
        zaura_shell_get_aura_output(ctx->aura_shell->internal, host->proxy);
    zaura_output_set_user_data(host->aura_output, host);
    zaura_output_add_listener(host->aura_output, &sl_aura_output_listener,
                              host);
  }
}

struct sl_global* sl_output_global_create(struct sl_output* output) {
  return sl_global_create(output->ctx, &wl_output_interface, output->version,
                          output, sl_bind_host_output);
}
