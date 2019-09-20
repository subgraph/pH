// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "sommelier.h"

#include <assert.h>
#include <stdlib.h>
#include <wayland-client.h>

struct sl_host_shell_surface {
  struct wl_resource* resource;
  struct wl_shell_surface* proxy;
};

struct sl_host_shell {
  struct sl_shell* shell;
  struct wl_resource* resource;
  struct wl_shell* proxy;
};

static void sl_shell_surface_pong(struct wl_client* client,
                                  struct wl_resource* resource,
                                  uint32_t serial) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);

  wl_shell_surface_pong(host->proxy, serial);
}

static void sl_shell_surface_move(struct wl_client* client,
                                  struct wl_resource* resource,
                                  struct wl_resource* seat_resource,
                                  uint32_t serial) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_seat* host_seat = wl_resource_get_user_data(seat_resource);

  wl_shell_surface_move(host->proxy, host_seat->proxy, serial);
}

static void sl_shell_surface_resize(struct wl_client* client,
                                    struct wl_resource* resource,
                                    struct wl_resource* seat_resource,
                                    uint32_t serial,
                                    uint32_t edges) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_seat* host_seat = wl_resource_get_user_data(seat_resource);

  wl_shell_surface_resize(host->proxy, host_seat->proxy, serial, edges);
}

static void sl_shell_surface_set_toplevel(struct wl_client* client,
                                          struct wl_resource* resource) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);

  wl_shell_surface_set_toplevel(host->proxy);
}

static void sl_shell_surface_set_transient(struct wl_client* client,
                                           struct wl_resource* resource,
                                           struct wl_resource* parent_resource,
                                           int32_t x,
                                           int32_t y,
                                           uint32_t flags) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_surface* host_parent =
      wl_resource_get_user_data(parent_resource);

  wl_shell_surface_set_transient(host->proxy, host_parent->proxy, x, y, flags);
}

static void sl_shell_surface_set_fullscreen(
    struct wl_client* client,
    struct wl_resource* resource,
    uint32_t method,
    uint32_t framerate,
    struct wl_resource* output_resource) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_output* host_output =
      output_resource ? wl_resource_get_user_data(output_resource) : NULL;

  wl_shell_surface_set_fullscreen(host->proxy, method, framerate,
                                  host_output ? host_output->proxy : NULL);
}

static void sl_shell_surface_set_popup(struct wl_client* client,
                                       struct wl_resource* resource,
                                       struct wl_resource* seat_resource,
                                       uint32_t serial,
                                       struct wl_resource* parent_resource,
                                       int32_t x,
                                       int32_t y,
                                       uint32_t flags) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_seat* host_seat = wl_resource_get_user_data(seat_resource);
  struct sl_host_surface* host_parent =
      wl_resource_get_user_data(parent_resource);

  wl_shell_surface_set_popup(host->proxy, host_seat->proxy, serial,
                             host_parent->proxy, x, y, flags);
}

static void sl_shell_surface_set_maximized(
    struct wl_client* client,
    struct wl_resource* resource,
    struct wl_resource* output_resource) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);
  struct sl_host_output* host_output =
      output_resource ? wl_resource_get_user_data(output_resource) : NULL;

  wl_shell_surface_set_maximized(host->proxy,
                                 host_output ? host_output->proxy : NULL);
}

static void sl_shell_surface_set_title(struct wl_client* client,
                                       struct wl_resource* resource,
                                       const char* title) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);

  wl_shell_surface_set_title(host->proxy, title);
}

static void sl_shell_surface_set_class(struct wl_client* client,
                                       struct wl_resource* resource,
                                       const char* clazz) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);

  wl_shell_surface_set_class(host->proxy, clazz);
}

static const struct wl_shell_surface_interface sl_shell_surface_implementation =
    {sl_shell_surface_pong,          sl_shell_surface_move,
     sl_shell_surface_resize,        sl_shell_surface_set_toplevel,
     sl_shell_surface_set_transient, sl_shell_surface_set_fullscreen,
     sl_shell_surface_set_popup,     sl_shell_surface_set_maximized,
     sl_shell_surface_set_title,     sl_shell_surface_set_class};

static void sl_shell_surface_ping(void* data,
                                  struct wl_shell_surface* shell_surface,
                                  uint32_t serial) {
  struct sl_host_shell_surface* host =
      wl_shell_surface_get_user_data(shell_surface);

  wl_shell_surface_send_ping(host->resource, serial);
}

static void sl_shell_surface_configure(void* data,
                                       struct wl_shell_surface* shell_surface,
                                       uint32_t edges,
                                       int32_t width,
                                       int32_t height) {
  struct sl_host_shell_surface* host =
      wl_shell_surface_get_user_data(shell_surface);

  wl_shell_surface_send_configure(host->resource, edges, width, height);
}

static void sl_shell_surface_popup_done(
    void* data, struct wl_shell_surface* shell_surface) {
  struct sl_host_shell_surface* host =
      wl_shell_surface_get_user_data(shell_surface);

  wl_shell_surface_send_popup_done(host->resource);
}

static const struct wl_shell_surface_listener sl_shell_surface_listener = {
    sl_shell_surface_ping, sl_shell_surface_configure,
    sl_shell_surface_popup_done};

static void sl_destroy_host_shell_surface(struct wl_resource* resource) {
  struct sl_host_shell_surface* host = wl_resource_get_user_data(resource);

  wl_shell_surface_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_host_shell_get_shell_surface(
    struct wl_client* client,
    struct wl_resource* resource,
    uint32_t id,
    struct wl_resource* surface_resource) {
  struct sl_host_shell* host = wl_resource_get_user_data(resource);
  struct sl_host_surface* host_surface =
      wl_resource_get_user_data(surface_resource);
  struct sl_host_shell_surface* host_shell_surface;

  host_shell_surface = malloc(sizeof(*host_shell_surface));
  assert(host_shell_surface);
  host_shell_surface->resource =
      wl_resource_create(client, &wl_shell_surface_interface, 1, id);
  wl_resource_set_implementation(
      host_shell_surface->resource, &sl_shell_surface_implementation,
      host_shell_surface, sl_destroy_host_shell_surface);
  host_shell_surface->proxy =
      wl_shell_get_shell_surface(host->proxy, host_surface->proxy);
  wl_shell_surface_set_user_data(host_shell_surface->proxy, host_shell_surface);
  wl_shell_surface_add_listener(host_shell_surface->proxy,
                                &sl_shell_surface_listener, host_shell_surface);
  host_surface->has_role = 1;
}

static const struct wl_shell_interface sl_shell_implementation = {
    sl_host_shell_get_shell_surface};

static void sl_destroy_host_shell(struct wl_resource* resource) {
  struct sl_host_shell* host = wl_resource_get_user_data(resource);

  wl_shell_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_bind_host_shell(struct wl_client* client,
                               void* data,
                               uint32_t version,
                               uint32_t id) {
  struct sl_context* ctx = (struct sl_context*)data;
  struct sl_host_shell* host;

  host = malloc(sizeof(*host));
  assert(host);
  host->shell = ctx->shell;
  host->resource = wl_resource_create(client, &wl_shell_interface, 1, id);
  wl_resource_set_implementation(host->resource, &sl_shell_implementation, host,
                                 sl_destroy_host_shell);
  host->proxy = wl_registry_bind(wl_display_get_registry(ctx->display),
                                 ctx->shell->id, &wl_shell_interface,
                                 wl_resource_get_version(host->resource));
  wl_shell_set_user_data(host->proxy, host);
}

struct sl_global* sl_shell_global_create(struct sl_context* ctx) {
  return sl_global_create(ctx, &wl_shell_interface, 1, ctx, sl_bind_host_shell);
}