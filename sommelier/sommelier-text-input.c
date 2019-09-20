// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "sommelier.h"

#include <assert.h>
#include <stdlib.h>
#include <string.h>

#include "text-input-unstable-v1-client-protocol.h"
#include "text-input-unstable-v1-server-protocol.h"

struct sl_host_text_input_manager {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct zwp_text_input_manager_v1* proxy;
};

struct sl_host_text_input {
  struct sl_context* ctx;
  struct wl_resource* resource;
  struct zwp_text_input_v1* proxy;
};

static void sl_text_input_activate(struct wl_client* client,
                                   struct wl_resource* resource,
                                   struct wl_resource* seat,
                                   struct wl_resource* surface) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);
  struct sl_host_seat* host_seat = wl_resource_get_user_data(seat);
  struct sl_host_surface* host_surface = wl_resource_get_user_data(surface);

  zwp_text_input_v1_activate(host->proxy, host_seat->proxy,
                             host_surface->proxy);
}

static void sl_text_input_deactivate(struct wl_client* client,
                                     struct wl_resource* resource,
                                     struct wl_resource* seat) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);
  struct sl_host_seat* host_seat = wl_resource_get_user_data(seat);

  zwp_text_input_v1_deactivate(host->proxy, host_seat->proxy);
}

static void sl_text_input_show_input_panel(struct wl_client* client,
                                           struct wl_resource* resource) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);

  zwp_text_input_v1_show_input_panel(host->proxy);
}

static void sl_text_input_hide_input_panel(struct wl_client* client,
                                           struct wl_resource* resource) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);

  zwp_text_input_v1_hide_input_panel(host->proxy);
}

static void sl_text_input_reset(struct wl_client* client,
                                struct wl_resource* resource) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);

  zwp_text_input_v1_reset(host->proxy);
}

static void sl_text_input_set_surrounding_text(struct wl_client* client,
                                               struct wl_resource* resource,
                                               const char* text,
                                               uint32_t cursor,
                                               uint32_t anchor) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);

  zwp_text_input_v1_set_surrounding_text(host->proxy, text, cursor, anchor);
}

static void sl_text_input_set_content_type(struct wl_client* client,
                                           struct wl_resource* resource,
                                           uint32_t hint,
                                           uint32_t purpose) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);

  zwp_text_input_v1_set_content_type(host->proxy, hint, purpose);
}

static void sl_text_input_set_cursor_rectangle(struct wl_client* client,
                                               struct wl_resource* resource,
                                               int32_t x,
                                               int32_t y,
                                               int32_t width,
                                               int32_t height) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);

  zwp_text_input_v1_set_cursor_rectangle(host->proxy, x, y, width, height);
}

static void sl_text_input_set_preferred_language(struct wl_client* client,
                                                 struct wl_resource* resource,
                                                 const char* language) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);

  zwp_text_input_v1_set_preferred_language(host->proxy, language);
}

static void sl_text_input_commit_state(struct wl_client* client,
                                       struct wl_resource* resource,
                                       uint32_t serial) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);

  zwp_text_input_v1_commit_state(host->proxy, serial);
}

static void sl_text_input_invoke_action(struct wl_client* client,
                                        struct wl_resource* resource,
                                        uint32_t button,
                                        uint32_t index) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);

  zwp_text_input_v1_invoke_action(host->proxy, button, index);
}

static const struct zwp_text_input_v1_interface sl_text_input_implementation = {
    sl_text_input_activate,
    sl_text_input_deactivate,
    sl_text_input_show_input_panel,
    sl_text_input_hide_input_panel,
    sl_text_input_reset,
    sl_text_input_set_surrounding_text,
    sl_text_input_set_content_type,
    sl_text_input_set_cursor_rectangle,
    sl_text_input_set_preferred_language,
    sl_text_input_commit_state,
    sl_text_input_invoke_action,
};

static void sl_text_input_enter(void* data,
                                struct zwp_text_input_v1* text_input,
                                struct wl_surface* surface) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);
  struct sl_host_surface* host_surface = wl_surface_get_user_data(surface);

  zwp_text_input_v1_send_enter(host->resource, host_surface->resource);
}

static void sl_text_input_leave(void* data,
                                struct zwp_text_input_v1* text_input) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_leave(host->resource);
}

static void sl_text_input_modifiers_map(void* data,
                                        struct zwp_text_input_v1* text_input,
                                        struct wl_array* map) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_modifiers_map(host->resource, map);
}

static void sl_text_input_input_panel_state(
    void* data, struct zwp_text_input_v1* text_input, uint32_t state) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_input_panel_state(host->resource, state);
}

static void sl_text_input_preedit_string(void* data,
                                         struct zwp_text_input_v1* text_input,
                                         uint32_t serial,
                                         const char* text,
                                         const char* commit) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_preedit_string(host->resource, serial, text, commit);
}

static void sl_text_input_preedit_styling(void* data,
                                          struct zwp_text_input_v1* text_input,
                                          uint32_t index,
                                          uint32_t length,
                                          uint32_t style) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_preedit_styling(host->resource, index, length, style);
}

static void sl_text_input_preedit_cursor(void* data,
                                         struct zwp_text_input_v1* text_input,
                                         int32_t index) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_preedit_cursor(host->resource, index);
}

static void sl_text_input_commit_string(void* data,
                                        struct zwp_text_input_v1* text_input,
                                        uint32_t serial,
                                        const char* text) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_commit_string(host->resource, serial, text);
}

static void sl_text_input_cursor_position(void* data,
                                          struct zwp_text_input_v1* text_input,
                                          int32_t index,
                                          int32_t anchor) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_cursor_position(host->resource, index, anchor);
}

static void sl_text_input_delete_surrounding_text(
    void* data,
    struct zwp_text_input_v1* text_input,
    int32_t index,
    uint32_t length) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_delete_surrounding_text(host->resource, index, length);
}

static void sl_text_input_keysym(void* data,
                                 struct zwp_text_input_v1* text_input,
                                 uint32_t serial,
                                 uint32_t time,
                                 uint32_t sym,
                                 uint32_t state,
                                 uint32_t modifiers) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_keysym(host->resource, serial, time, sym, state,
                                modifiers);
}

static void sl_text_input_language(void* data,
                                   struct zwp_text_input_v1* text_input,
                                   uint32_t serial,
                                   const char* language) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_language(host->resource, serial, language);
}

static void sl_text_input_text_direction(void* data,
                                         struct zwp_text_input_v1* text_input,
                                         uint32_t serial,
                                         uint32_t direction) {
  struct sl_host_text_input* host = zwp_text_input_v1_get_user_data(text_input);

  zwp_text_input_v1_send_text_direction(host->resource, serial, direction);
}

static const struct zwp_text_input_v1_listener sl_text_input_listener = {
    sl_text_input_enter,           sl_text_input_leave,
    sl_text_input_modifiers_map,   sl_text_input_input_panel_state,
    sl_text_input_preedit_string,  sl_text_input_preedit_styling,
    sl_text_input_preedit_cursor,  sl_text_input_commit_string,
    sl_text_input_cursor_position, sl_text_input_delete_surrounding_text,
    sl_text_input_keysym,          sl_text_input_language,
    sl_text_input_text_direction,
};

static void sl_destroy_host_text_input(struct wl_resource* resource) {
  struct sl_host_text_input* host = wl_resource_get_user_data(resource);

  zwp_text_input_v1_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_text_input_manager_create_text_input(
    struct wl_client* client, struct wl_resource* resource, uint32_t id) {
  struct sl_host_text_input_manager* host = wl_resource_get_user_data(resource);
  struct wl_resource* text_input_resource =
      wl_resource_create(client, &zwp_text_input_v1_interface, 1, id);
  struct sl_host_text_input* text_input_host =
      malloc(sizeof(struct sl_host_text_input));

  text_input_host->resource = text_input_resource;
  text_input_host->ctx = host->ctx;
  text_input_host->proxy = zwp_text_input_manager_v1_create_text_input(
      host->ctx->text_input_manager->internal);
  wl_resource_set_implementation(text_input_resource,
                                 &sl_text_input_implementation, text_input_host,
                                 sl_destroy_host_text_input);
  zwp_text_input_v1_set_user_data(text_input_host->proxy, text_input_host);
  zwp_text_input_v1_add_listener(text_input_host->proxy,
                                 &sl_text_input_listener, text_input_host);
}

static void sl_destroy_host_text_input_manager(struct wl_resource* resource) {
  struct sl_host_text_input_manager* host = wl_resource_get_user_data(resource);

  zwp_text_input_manager_v1_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static struct zwp_text_input_manager_v1_interface
    sl_text_input_manager_implementation = {
        sl_text_input_manager_create_text_input,
};

static void sl_bind_host_text_input_manager(struct wl_client* client,
                                            void* data,
                                            uint32_t version,
                                            uint32_t id) {
  struct sl_context* ctx = (struct sl_context*)data;
  struct sl_text_input_manager* text_input_manager = ctx->text_input_manager;
  struct sl_host_text_input_manager* host;

  host = malloc(sizeof(*host));
  assert(host);
  host->ctx = ctx;
  host->resource =
      wl_resource_create(client, &zwp_text_input_manager_v1_interface, 1, id);
  wl_resource_set_implementation(host->resource,
                                 &sl_text_input_manager_implementation, host,
                                 sl_destroy_host_text_input_manager);
  host->proxy = wl_registry_bind(wl_display_get_registry(ctx->display),
                                 text_input_manager->id,
                                 &zwp_text_input_manager_v1_interface,
                                 wl_resource_get_version(host->resource));
  zwp_text_input_manager_v1_set_user_data(host->proxy, host);
}

struct sl_global* sl_text_input_manager_global_create(struct sl_context* ctx) {
  return sl_global_create(ctx, &zwp_text_input_manager_v1_interface, 1, ctx,
                          sl_bind_host_text_input_manager);
}
