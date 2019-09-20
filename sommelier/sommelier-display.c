// Copyright 2018 The Chromium OS Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "sommelier.h"

#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <wayland-client.h>

static void sl_registry_bind(struct wl_client* client,
                             struct wl_resource* resource,
                             uint32_t name,
                             const char* interface,
                             uint32_t version,
                             uint32_t id) {
  struct sl_host_registry* host = wl_resource_get_user_data(resource);
  struct sl_global* global;

  wl_list_for_each(global, &host->ctx->globals, link) {
    if (global->name == name)
      break;
  }

  assert(&global->link != &host->ctx->globals);
  assert(version != 0);
  assert(global->version >= version);

  global->bind(client, global->data, version, id);
}

static const struct wl_registry_interface sl_registry_implementation = {
    sl_registry_bind};

static void sl_sync_callback_done(void* data,
                                  struct wl_callback* callback,
                                  uint32_t serial) {
  struct sl_host_callback* host = wl_callback_get_user_data(callback);

  wl_callback_send_done(host->resource, serial);
  wl_resource_destroy(host->resource);
}

static const struct wl_callback_listener sl_sync_callback_listener = {
    sl_sync_callback_done};

static void sl_host_callback_destroy(struct wl_resource* resource) {
  struct sl_host_callback* host = wl_resource_get_user_data(resource);

  wl_callback_destroy(host->proxy);
  wl_resource_set_user_data(resource, NULL);
  free(host);
}

static void sl_display_sync(struct wl_client* client,
                            struct wl_resource* resource,
                            uint32_t id) {
  struct sl_context* ctx = wl_resource_get_user_data(resource);
  struct sl_host_callback* host_callback;

  host_callback = malloc(sizeof(*host_callback));
  assert(host_callback);

  host_callback->resource =
      wl_resource_create(client, &wl_callback_interface, 1, id);
  wl_resource_set_implementation(host_callback->resource, NULL, host_callback,
                                 sl_host_callback_destroy);
  host_callback->proxy = wl_display_sync(ctx->display);
  wl_callback_set_user_data(host_callback->proxy, host_callback);
  wl_callback_add_listener(host_callback->proxy, &sl_sync_callback_listener,
                           host_callback);
}

static void sl_destroy_host_registry(struct wl_resource* resource) {
  struct sl_host_registry* host = wl_resource_get_user_data(resource);

  wl_list_remove(&host->link);
  free(host);
}

static void sl_display_get_registry(struct wl_client* client,
                                    struct wl_resource* resource,
                                    uint32_t id) {
  struct sl_context* ctx = wl_resource_get_user_data(resource);
  struct sl_host_registry* host_registry;
  struct sl_global* global;

  host_registry = malloc(sizeof(*host_registry));
  assert(host_registry);

  host_registry->ctx = ctx;
  host_registry->resource =
      wl_resource_create(client, &wl_registry_interface, 1, id);
  wl_list_insert(&ctx->registries, &host_registry->link);
  wl_resource_set_implementation(host_registry->resource,
                                 &sl_registry_implementation, host_registry,
                                 sl_destroy_host_registry);

  wl_list_for_each(global, &ctx->globals, link) {
    wl_resource_post_event(host_registry->resource, WL_REGISTRY_GLOBAL,
                           global->name, global->interface->name,
                           global->version);
  }
}

static const struct wl_display_interface sl_display_implementation = {
    sl_display_sync, sl_display_get_registry};

static enum wl_iterator_result sl_set_implementation(
    struct wl_resource* resource, void* user_data) {
  struct sl_context* ctx = (struct sl_context*)user_data;

  if (strcmp(wl_resource_get_class(resource), "wl_display") == 0) {
    wl_resource_set_implementation(resource, &sl_display_implementation, ctx,
                                   NULL);
    return WL_ITERATOR_STOP;
  }

  return WL_ITERATOR_CONTINUE;
}

void sl_set_display_implementation(struct sl_context* ctx) {
  // Find display resource and set implementation.
  wl_client_for_each_resource(ctx->client, sl_set_implementation, ctx);
}
