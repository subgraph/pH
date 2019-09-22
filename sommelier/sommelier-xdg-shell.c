// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "sommelier.h"

#include <assert.h>
#include <stdlib.h>

#include "xdg-shell-unstable-v6-client-protocol.h"
#include "xdg-shell-unstable-v6-server-protocol.h"

struct sl_host_xdg_shell {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct zxdg_shell_v6* proxy;
};

struct sl_host_xdg_surface {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct zxdg_surface_v6* proxy;
};

struct sl_host_xdg_toplevel {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct zxdg_toplevel_v6* proxy;
};

struct sl_host_xdg_popup {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct zxdg_popup_v6* proxy;
};

struct sl_host_xdg_positioner {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct zxdg_positioner_v6* proxy;
};

static void sl_xdg_positioner_destroy(struct wl_client* client,
                                      struct wl_resource* resource) {
  wl_resource_destroy(resource);
}

static void sl_xdg_positioner_set_size(struct wl_client* client,
                                       struct wl_resource* resource,
                                       int32_t width,
                                       int32_t height) {
  struct sl_host_xdg_positioner* host = wl_resource_get_user_data(resource);
  double scale = host->ctx->scale;

  zxdg_positioner_v6_set_size(host->proxy, width / scale, height / scale);
}

static void sl_xdg_positioner_set_anchor_rect(struct wl_client* client,
                                              struct wl_resource* resource,
                                              int32_t x,
                                              int32_t y,
                                              int32_t width,
                                              int32_t height) {
  struct sl_host_xdg_positioner* host = wl_resource_get_user_data(resource);
  double scale = host->ctx->scale;
  int32_t x1, y1, x2, y2;

  x1 = x / scale;
  y1 = y / scale;
  x2 = (x + width) / scale;
  y2 = (y + height) / scale;

  zxdg_positioner_v6_set_anchor_rect(host->proxy, x1, y1, x2 - x1, y2 - y1);
}

static void sl_xdg_positioner_set_anchor(struct wl_client* client,
                                         struct wl_resource* resource,
                                         uint32_t anchor) {
  struct sl_host_xdg_positioner* host = wl_resource_get_user_data(resource);

  zxdg_positioner_v6_set_anchor(host->proxy, anchor);
}

static void sl_xdg_positioner_set_gravity(struct wl_client* client,
                                          struct wl_resource* resource,
                                          uint32_t gravity) {
  struct sl_host_xdg_positioner* host = wl_resource_get_user_data(resource);

  zxdg_positioner_v6_set_gravity(host->proxy, gravity);
}

static void sl_xdg_positioner_set_constraint_adjustment(
    struct wl_client* client,
    struct wl_resource* resource,
    uint32_t constraint_adjustment) {
  struct sl_host_xdg_positioner* host = wl_resource_get_user_data(resource);

  zxdg_positioner_v6_set_constraint_adjustment(host->proxy,
                                               constraint_adjustment);
}

static void sl_xdg_positioner_set_offset(struct wl_client* client,
                                         struct wl_resource* resource,
                                         int32_t x,
                                         int32_t y) {
  struct sl_host_xdg_positioner* host = wl_resource_get_user_data(resource);
  double scale = host->ctx->scale;

  zxdg_positioner_v6_set_offset(host->proxy, x / scale, y / scale);
}

static const struct zxdg_positioner_v6_interface
    sl_xdg_positioner_implementation = {
        sl_xdg_positioner_destroy,
        sl_xdg_positioner_set_size,
        sl_xdg_positioner_set_anchor_rect,
        sl_xdg_positioner_set_anchor,
        sl_xdg_positioner_set_gravity,
        sl_xdg_positioner_set_constraint_adjustment,
        sl_xdg_positioner_set_offset};

static void sl_destroy_host_xdg_positioner(struct wl_resource* resource) {
  struct sl_host_xdg_positioner* host = wl_resource_get_user_data(resource);

  zxdg_positioner_v6_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_xdg_popup_destroy(struct wl_client* client,
                                 struct wl_resource* resource) {
  wl_resource_destroy(resource);
}

static void sl_xdg_popup_grab(struct wl_client* client,
                              struct wl_resource* resource,
                              struct wl_resource* seat_resource,
                              uint32_t serial) {
  struct sl_host_xdg_popup* host = wl_resource_get_user_data(resource);
  struct sl_host_seat* host_seat = wl_resource_get_user_data(seat_resource);

  zxdg_popup_v6_grab(host->proxy, host_seat->proxy, serial);
}

static const struct zxdg_popup_v6_interface sl_xdg_popup_implementation = {
    sl_xdg_popup_destroy, sl_xdg_popup_grab};

static void sl_xdg_popup_configure(void* data,
                                   struct zxdg_popup_v6* xdg_popup,
                                   int32_t x,
                                   int32_t y,
                                   int32_t width,
                                   int32_t height) {
  struct sl_host_xdg_popup* host = zxdg_popup_v6_get_user_data(xdg_popup);
  double scale = host->ctx->scale;
  int32_t x1, y1, x2, y2;

  x1 = x * scale;
  y1 = y * scale;
  x2 = (x + width) * scale;
  y2 = (y + height) * scale;

  zxdg_popup_v6_send_configure(host->resource, x1, y1, x2 - x1, y2 - y1);
}

static void sl_xdg_popup_popup_done(void* data,
                                    struct zxdg_popup_v6* xdg_popup) {
  struct sl_host_xdg_popup* host = zxdg_popup_v6_get_user_data(xdg_popup);

  zxdg_popup_v6_send_popup_done(host->resource);
}

static const struct zxdg_popup_v6_listener sl_xdg_popup_listener = {
    sl_xdg_popup_configure, sl_xdg_popup_popup_done};

static void sl_destroy_host_xdg_popup(struct wl_resource* resource) {
  struct sl_host_xdg_popup* host = wl_resource_get_user_data(resource);

  zxdg_popup_v6_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_xdg_toplevel_destroy(struct wl_client* client,
                                    struct wl_resource* resource) {
  wl_resource_destroy(resource);
}

static void sl_xdg_toplevel_set_parent(struct wl_client* client,
                                       struct wl_resource* resource,
                                       struct wl_resource* parent_resource) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);
  struct sl_host_xdg_toplevel* host_parent =
      parent_resource ? wl_resource_get_user_data(parent_resource) : NULL;

  zxdg_toplevel_v6_set_parent(host->proxy,
                              host_parent ? host_parent->proxy : NULL);
}

static void sl_xdg_toplevel_set_title(struct wl_client* client,
                                      struct wl_resource* resource,
                                      const char* title) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);

  zxdg_toplevel_v6_set_title(host->proxy, title);
}

static void sl_xdg_toplevel_set_app_id(struct wl_client* client,
                                       struct wl_resource* resource,
                                       const char* app_id) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);

  zxdg_toplevel_v6_set_app_id(host->proxy, app_id);
}

static void sl_xdg_toplevel_show_window_menu(struct wl_client* client,
                                             struct wl_resource* resource,
                                             struct wl_resource* seat_resource,
                                             uint32_t serial,
                                             int32_t x,
                                             int32_t y) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);
  struct sl_host_seat* host_seat =
      seat_resource ? wl_resource_get_user_data(seat_resource) : NULL;

  zxdg_toplevel_v6_show_window_menu(
      host->proxy, host_seat ? host_seat->proxy : NULL, serial, x, y);
}

static void sl_xdg_toplevel_move(struct wl_client* client,
                                 struct wl_resource* resource,
                                 struct wl_resource* seat_resource,
                                 uint32_t serial) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);
  struct sl_host_seat* host_seat =
      seat_resource ? wl_resource_get_user_data(seat_resource) : NULL;

  zxdg_toplevel_v6_move(host->proxy, host_seat ? host_seat->proxy : NULL,
                        serial);
}

static void sl_xdg_toplevel_resize(struct wl_client* client,
                                   struct wl_resource* resource,
                                   struct wl_resource* seat_resource,
                                   uint32_t serial,
                                   uint32_t edges) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);
  struct sl_host_seat* host_seat =
      seat_resource ? wl_resource_get_user_data(seat_resource) : NULL;

  zxdg_toplevel_v6_resize(host->proxy, host_seat ? host_seat->proxy : NULL,
                          serial, edges);
}

static void sl_xdg_toplevel_set_max_size(struct wl_client* client,
                                         struct wl_resource* resource,
                                         int32_t width,
                                         int32_t height) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);

  zxdg_toplevel_v6_set_max_size(host->proxy, width, height);
}

static void sl_xdg_toplevel_set_min_size(struct wl_client* client,
                                         struct wl_resource* resource,
                                         int32_t width,
                                         int32_t height) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);

  zxdg_toplevel_v6_set_min_size(host->proxy, width, height);
}

static void sl_xdg_toplevel_set_maximized(struct wl_client* client,
                                          struct wl_resource* resource) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);

  zxdg_toplevel_v6_set_maximized(host->proxy);
}

static void sl_xdg_toplevel_unset_maximized(struct wl_client* client,
                                            struct wl_resource* resource) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);

  zxdg_toplevel_v6_unset_maximized(host->proxy);
}

static void sl_xdg_toplevel_set_fullscreen(
    struct wl_client* client,
    struct wl_resource* resource,
    struct wl_resource* output_resource) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);
  struct sl_host_output* host_output =
      output_resource ? wl_resource_get_user_data(output_resource) : NULL;

  zxdg_toplevel_v6_set_fullscreen(host->proxy,
                                  host_output ? host_output->proxy : NULL);
}

static void sl_xdg_toplevel_unset_fullscreen(struct wl_client* client,
                                             struct wl_resource* resource) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);

  zxdg_toplevel_v6_unset_fullscreen(host->proxy);
}

static void sl_xdg_toplevel_set_minimized(struct wl_client* client,
                                          struct wl_resource* resource) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);

  zxdg_toplevel_v6_set_minimized(host->proxy);
}

static const struct zxdg_toplevel_v6_interface sl_xdg_toplevel_implementation =
    {sl_xdg_toplevel_destroy,          sl_xdg_toplevel_set_parent,
     sl_xdg_toplevel_set_title,        sl_xdg_toplevel_set_app_id,
     sl_xdg_toplevel_show_window_menu, sl_xdg_toplevel_move,
     sl_xdg_toplevel_resize,           sl_xdg_toplevel_set_max_size,
     sl_xdg_toplevel_set_min_size,     sl_xdg_toplevel_set_maximized,
     sl_xdg_toplevel_unset_maximized,  sl_xdg_toplevel_set_fullscreen,
     sl_xdg_toplevel_unset_fullscreen, sl_xdg_toplevel_set_minimized};

static void sl_xdg_toplevel_configure(void* data,
                                      struct zxdg_toplevel_v6* xdg_toplevel,
                                      int32_t width,
                                      int32_t height,
                                      struct wl_array* states) {
  struct sl_host_xdg_toplevel* host =
      zxdg_toplevel_v6_get_user_data(xdg_toplevel);
  double scale = host->ctx->scale;

  zxdg_toplevel_v6_send_configure(host->resource, width * scale, height * scale,
                                  states);
}

static void sl_xdg_toplevel_close(void* data,
                                  struct zxdg_toplevel_v6* xdg_toplevel) {
  struct sl_host_xdg_toplevel* host =
      zxdg_toplevel_v6_get_user_data(xdg_toplevel);

  zxdg_toplevel_v6_send_close(host->resource);
}

static const struct zxdg_toplevel_v6_listener sl_xdg_toplevel_listener = {
    sl_xdg_toplevel_configure, sl_xdg_toplevel_close};

static void sl_destroy_host_xdg_toplevel(struct wl_resource* resource) {
  struct sl_host_xdg_toplevel* host = wl_resource_get_user_data(resource);

  zxdg_toplevel_v6_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_xdg_surface_destroy(struct wl_client* client,
                                   struct wl_resource* resource) {
  wl_resource_destroy(resource);
}

static void sl_xdg_surface_get_toplevel(struct wl_client* client,
                                        struct wl_resource* resource,
                                        uint32_t id) {
  struct sl_host_xdg_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_xdg_toplevel* host_xdg_toplevel;

  host_xdg_toplevel = malloc(sizeof(*host_xdg_toplevel));
  assert(host_xdg_toplevel);

  host_xdg_toplevel->ctx = host->ctx;
  host_xdg_toplevel->resource =
      wl_resource_create(client, &zxdg_toplevel_v6_interface, 1, id);
  wl_resource_set_implementation(
      host_xdg_toplevel->resource, &sl_xdg_toplevel_implementation,
      host_xdg_toplevel, sl_destroy_host_xdg_toplevel);
  host_xdg_toplevel->proxy = zxdg_surface_v6_get_toplevel(host->proxy);
  zxdg_toplevel_v6_set_user_data(host_xdg_toplevel->proxy, host_xdg_toplevel);
  zxdg_toplevel_v6_add_listener(host_xdg_toplevel->proxy,
                                &sl_xdg_toplevel_listener, host_xdg_toplevel);
}

static void sl_xdg_surface_get_popup(struct wl_client* client,
                                     struct wl_resource* resource,
                                     uint32_t id,
                                     struct wl_resource* parent_resource,
                                     struct wl_resource* positioner_resource) {
  struct sl_host_xdg_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_xdg_surface* host_parent =
      wl_resource_get_user_data(parent_resource);
  struct sl_host_xdg_positioner* host_positioner =
      wl_resource_get_user_data(positioner_resource);
  struct sl_host_xdg_popup* host_xdg_popup;

  host_xdg_popup = malloc(sizeof(*host_xdg_popup));
  assert(host_xdg_popup);

  host_xdg_popup->ctx = host->ctx;
  host_xdg_popup->resource =
      wl_resource_create(client, &zxdg_popup_v6_interface, 1, id);
  wl_resource_set_implementation(host_xdg_popup->resource,
                                 &sl_xdg_popup_implementation, host_xdg_popup,
                                 sl_destroy_host_xdg_popup);
  host_xdg_popup->proxy = zxdg_surface_v6_get_popup(
      host->proxy, host_parent->proxy, host_positioner->proxy);
  zxdg_popup_v6_set_user_data(host_xdg_popup->proxy, host_xdg_popup);
  zxdg_popup_v6_add_listener(host_xdg_popup->proxy, &sl_xdg_popup_listener,
                             host_xdg_popup);
}

static void sl_xdg_surface_set_window_geometry(struct wl_client* client,
                                               struct wl_resource* resource,
                                               int32_t x,
                                               int32_t y,
                                               int32_t width,
                                               int32_t height) {
  struct sl_host_xdg_surface* host = wl_resource_get_user_data(resource);
  double scale = host->ctx->scale;
  int32_t x1, y1, x2, y2;

  x1 = x / scale;
  y1 = y / scale;
  x2 = (x + width) / scale;
  y2 = (y + height) / scale;

  zxdg_surface_v6_set_window_geometry(host->proxy, x1, y1, x2 - x1, y2 - y1);
}

static void sl_xdg_surface_ack_configure(struct wl_client* client,
                                         struct wl_resource* resource,
                                         uint32_t serial) {
  struct sl_host_xdg_surface* host = wl_resource_get_user_data(resource);

  zxdg_surface_v6_ack_configure(host->proxy, serial);
}

static const struct zxdg_surface_v6_interface sl_xdg_surface_implementation = {
    sl_xdg_surface_destroy, sl_xdg_surface_get_toplevel,
    sl_xdg_surface_get_popup, sl_xdg_surface_set_window_geometry,
    sl_xdg_surface_ack_configure};

static void sl_xdg_surface_configure(void* data,
                                     struct zxdg_surface_v6* xdg_surface,
                                     uint32_t serial) {
  struct sl_host_xdg_surface* host = zxdg_surface_v6_get_user_data(xdg_surface);

  zxdg_surface_v6_send_configure(host->resource, serial);
}

static const struct zxdg_surface_v6_listener sl_xdg_surface_listener = {
    sl_xdg_surface_configure};

static void sl_destroy_host_xdg_surface(struct wl_resource* resource) {
  struct sl_host_xdg_surface* host = wl_resource_get_user_data(resource);

  zxdg_surface_v6_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_xdg_shell_destroy(struct wl_client* client,
                                 struct wl_resource* resource) {
  wl_resource_destroy(resource);
}

static void sl_xdg_shell_create_positioner(struct wl_client* client,
                                           struct wl_resource* resource,
                                           uint32_t id) {
  struct sl_host_xdg_shell* host = wl_resource_get_user_data(resource);
  struct sl_host_xdg_positioner* host_xdg_positioner;

  host_xdg_positioner = malloc(sizeof(*host_xdg_positioner));
  assert(host_xdg_positioner);

  host_xdg_positioner->ctx = host->ctx;
  host_xdg_positioner->resource =
      wl_resource_create(client, &zxdg_positioner_v6_interface, 1, id);
  wl_resource_set_implementation(
      host_xdg_positioner->resource, &sl_xdg_positioner_implementation,
      host_xdg_positioner, sl_destroy_host_xdg_positioner);
  host_xdg_positioner->proxy = zxdg_shell_v6_create_positioner(host->proxy);
  zxdg_positioner_v6_set_user_data(host_xdg_positioner->proxy,
                                   host_xdg_positioner);
}

static void sl_xdg_shell_get_xdg_surface(struct wl_client* client,
                                         struct wl_resource* resource,
                                         uint32_t id,
                                         struct wl_resource* surface_resource) {
  struct sl_host_xdg_shell* host = wl_resource_get_user_data(resource);
  struct sl_host_surface* host_surface =
      wl_resource_get_user_data(surface_resource);
  struct sl_host_xdg_surface* host_xdg_surface;

  host_xdg_surface = malloc(sizeof(*host_xdg_surface));
  assert(host_xdg_surface);

  host_xdg_surface->ctx = host->ctx;
  host_xdg_surface->resource =
      wl_resource_create(client, &zxdg_surface_v6_interface, 1, id);
  wl_resource_set_implementation(host_xdg_surface->resource,
                                 &sl_xdg_surface_implementation,
                                 host_xdg_surface, sl_destroy_host_xdg_surface);
  host_xdg_surface->proxy =
      zxdg_shell_v6_get_xdg_surface(host->proxy, host_surface->proxy);
  zxdg_surface_v6_set_user_data(host_xdg_surface->proxy, host_xdg_surface);
  zxdg_surface_v6_add_listener(host_xdg_surface->proxy,
                               &sl_xdg_surface_listener, host_xdg_surface);
  host_surface->has_role = 1;
}

static void sl_xdg_shell_pong(struct wl_client* client,
                              struct wl_resource* resource,
                              uint32_t serial) {
  struct sl_host_xdg_shell* host = wl_resource_get_user_data(resource);

  zxdg_shell_v6_pong(host->proxy, serial);
}

static const struct zxdg_shell_v6_interface sl_xdg_shell_implementation = {
    sl_xdg_shell_destroy, sl_xdg_shell_create_positioner,
    sl_xdg_shell_get_xdg_surface, sl_xdg_shell_pong};

static void sl_xdg_shell_ping(void* data,
                              struct zxdg_shell_v6* xdg_shell,
                              uint32_t serial) {
  struct sl_host_xdg_shell* host = zxdg_shell_v6_get_user_data(xdg_shell);

  zxdg_shell_v6_send_ping(host->resource, serial);
}

static const struct zxdg_shell_v6_listener sl_xdg_shell_listener = {
    sl_xdg_shell_ping};

static void sl_destroy_host_xdg_shell(struct wl_resource* resource) {
  struct sl_host_xdg_shell* host = wl_resource_get_user_data(resource);

  zxdg_shell_v6_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_bind_host_xdg_shell(struct wl_client* client,
                                   void* data,
                                   uint32_t version,
                                   uint32_t id) {
  struct sl_context* ctx = (struct sl_context*)data;
  struct sl_host_xdg_shell* host;

  host = malloc(sizeof(*host));
  assert(host);
  host->ctx = ctx;
  host->resource = wl_resource_create(client, &zxdg_shell_v6_interface, 1, id);
  wl_resource_set_implementation(host->resource, &sl_xdg_shell_implementation,
                                 host, sl_destroy_host_xdg_shell);
  host->proxy =
      wl_registry_bind(wl_display_get_registry(ctx->display),
                       ctx->xdg_shell->id, &zxdg_shell_v6_interface, 1);
  zxdg_shell_v6_set_user_data(host->proxy, host);
  zxdg_shell_v6_add_listener(host->proxy, &sl_xdg_shell_listener, host);
}

struct sl_global* sl_xdg_shell_global_create(struct sl_context* ctx) {
  return sl_global_create(ctx, &zxdg_shell_v6_interface, 1, ctx,
                          sl_bind_host_xdg_shell);
}
