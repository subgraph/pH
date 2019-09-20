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

// The ergonomic advice for monitor distance is 50-75cm away, with laptops
// expected to be closer. This magic number is designed to correct that for the
// purpose of calculating a "useful" DPI.
//
// TODO(crbug.com/988325) Fix sommelier's scaling logic s.t. this ratio is
// unnecessary.
#define LAPTOP_TO_DESKTOP_DISTANCE_RATIO (2.0 / 3.0)

double sl_output_aura_scale_factor_to_double(int scale_factor) {
  // Aura scale factor is an enum that for all currently know values
  // is a scale value multipled by 1000. For example, enum value for
  // 1.25 scale factor is 1250.
  return scale_factor / 1000.0;
}

void sl_output_get_host_output_state(struct sl_host_output* host,
                                     int* scale,
                                     int* physical_width,
                                     int* physical_height,
                                     int* width,
                                     int* height) {
  double preferred_scale =
      sl_output_aura_scale_factor_to_double(host->preferred_scale);
  double current_scale =
      sl_output_aura_scale_factor_to_double(host->current_scale);
  // "Ideal" means the scale factor you would need in order to make a pixel in
  // the buffer map 1:1 with a physical pixel. In the absence of any better
  // information, we assume a device whose display density maps faithfully to
  // true pixels (i.e. 1.0).
  double ideal_scale_factor = 1.0;
  double scale_factor = host->scale_factor;

  // Use the scale factor we received from aura shell protocol when available.
  if (host->ctx->aura_shell) {
    double device_scale_factor =
        sl_output_aura_scale_factor_to_double(host->device_scale_factor);

    ideal_scale_factor = device_scale_factor * preferred_scale;
    scale_factor = device_scale_factor * current_scale;
  }

  // Always use scale=1 and adjust geometry and mode based on ideal
  // scale factor for Xwayland client. For other clients, pick an optimal
  // scale and adjust geometry and mode based on it.
  if (host->ctx->xwayland) {
    if (scale)
      *scale = 1;
    *physical_width = host->physical_width * ideal_scale_factor / scale_factor;
    *physical_height =
        host->physical_height * ideal_scale_factor / scale_factor;
    *width = host->width * host->ctx->scale / scale_factor;
    *height = host->height * host->ctx->scale / scale_factor;

    // Historically, X applications use DPI to decide their scale (which is not
    // ideal). The main problem is that in order to facilitate this, many X
    // utilities lie about the DPI of the device in order to achieve the desired
    // scaling, e.g. most laptops report a dpi of 96 even if that is inaccurate.
    //
    // The reason they have to lie is because laptop screens are typically
    // closer to your eye than desktop monitors (by a factor of roughly 2/3),
    // meaning they have to have proportionally higher DPI in order to "look" as
    // high-def as the monitor.
    //
    // Since sommelier is in the business of lying about the screen's
    // dimensions, we will also lie a bit more when we are dealing with the
    // internal display, to make its dpi scale like a desktop monitor's would.
    if (host->internal) {
      *physical_width /= LAPTOP_TO_DESKTOP_DISTANCE_RATIO;
      *physical_height /= LAPTOP_TO_DESKTOP_DISTANCE_RATIO;
    }
  } else {
    int s = MIN(ceil(scale_factor / host->ctx->scale), MAX_OUTPUT_SCALE);

    if (scale)
      *scale = s;
    *physical_width = host->physical_width;
    *physical_height = host->physical_height;
    *width = host->width * host->ctx->scale * s / scale_factor;
    *height = host->height * host->ctx->scale * s / scale_factor;
  }

  if (host->ctx->dpi.size) {
    int dpi = (*width * INCH_IN_MM) / *physical_width;
    int adjusted_dpi = *((int*)host->ctx->dpi.data);
    double mmpd;
    int* p;

    // Choose the DPI bucket which is closest to the apparent DPI which we
    // calculated above.
    wl_array_for_each(p, &host->ctx->dpi) {
      if (abs(*p - dpi) < abs(adjusted_dpi - dpi))
        adjusted_dpi = *p;
    }

    mmpd = INCH_IN_MM / adjusted_dpi;
    *physical_width = *width * mmpd + 0.5;
    *physical_height = *height * mmpd + 0.5;
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

  switch (scale) {
    case ZAURA_OUTPUT_SCALE_FACTOR_0400:
    case ZAURA_OUTPUT_SCALE_FACTOR_0500:
    case ZAURA_OUTPUT_SCALE_FACTOR_0550:
    case ZAURA_OUTPUT_SCALE_FACTOR_0600:
    case ZAURA_OUTPUT_SCALE_FACTOR_0625:
    case ZAURA_OUTPUT_SCALE_FACTOR_0650:
    case ZAURA_OUTPUT_SCALE_FACTOR_0700:
    case ZAURA_OUTPUT_SCALE_FACTOR_0750:
    case ZAURA_OUTPUT_SCALE_FACTOR_0800:
    case ZAURA_OUTPUT_SCALE_FACTOR_0850:
    case ZAURA_OUTPUT_SCALE_FACTOR_0900:
    case ZAURA_OUTPUT_SCALE_FACTOR_0950:
    case ZAURA_OUTPUT_SCALE_FACTOR_1000:
    case ZAURA_OUTPUT_SCALE_FACTOR_1050:
    case ZAURA_OUTPUT_SCALE_FACTOR_1100:
    case ZAURA_OUTPUT_SCALE_FACTOR_1150:
    case ZAURA_OUTPUT_SCALE_FACTOR_1125:
    case ZAURA_OUTPUT_SCALE_FACTOR_1200:
    case ZAURA_OUTPUT_SCALE_FACTOR_1250:
    case ZAURA_OUTPUT_SCALE_FACTOR_1300:
    case ZAURA_OUTPUT_SCALE_FACTOR_1400:
    case ZAURA_OUTPUT_SCALE_FACTOR_1450:
    case ZAURA_OUTPUT_SCALE_FACTOR_1500:
    case ZAURA_OUTPUT_SCALE_FACTOR_1600:
    case ZAURA_OUTPUT_SCALE_FACTOR_1750:
    case ZAURA_OUTPUT_SCALE_FACTOR_1800:
    case ZAURA_OUTPUT_SCALE_FACTOR_2000:
    case ZAURA_OUTPUT_SCALE_FACTOR_2200:
    case ZAURA_OUTPUT_SCALE_FACTOR_2250:
    case ZAURA_OUTPUT_SCALE_FACTOR_2500:
    case ZAURA_OUTPUT_SCALE_FACTOR_2750:
    case ZAURA_OUTPUT_SCALE_FACTOR_3000:
    case ZAURA_OUTPUT_SCALE_FACTOR_3500:
    case ZAURA_OUTPUT_SCALE_FACTOR_4000:
    case ZAURA_OUTPUT_SCALE_FACTOR_4500:
    case ZAURA_OUTPUT_SCALE_FACTOR_5000:
      break;
    default:
      fprintf(stderr, "warning: unknown scale factor: %d\n", scale);
      break;
  }

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
