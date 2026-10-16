#ifndef OSG_PLATFORM_H
#define OSG_PLATFORM_H
#include "settings.h"
#include "esp_transport.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
typedef struct { esp_transport_handle_t tls, ws; unsigned char ca_der[1536]; size_t ca_bytes; bool direct; uint64_t serial_sequence; char link[32]; } osg_socket;
extern const osg_crypto osg_platform_crypto;
bool osg_trusted_ca(const osg_settings *, unsigned char *, size_t, size_t *);
bool osg_socket_open(osg_socket *,const osg_settings *);
void osg_socket_close(osg_socket *);
bool osg_socket_request(osg_socket *, cJSON *, cJSON **, unsigned timeout_ms);
bool osg_serial_open(osg_socket *);
void osg_serial_close(osg_socket *);
bool osg_serial_request(osg_socket *,cJSON *,cJSON **,unsigned);
cJSON *osg_serial_take_command(void);
cJSON *osg_wifi_scan(void);
bool osg_wifi_clock(void);
bool osg_wifi_start(const osg_settings *);
void osg_wifi_stop(void);
bool osg_store_available(void);
bool osg_store_save(const osg_settings *);
bool osg_store_load(osg_settings *);
bool osg_store_forget(void);
typedef struct { osg_runtime *runtime; SemaphoreHandle_t mutex; bool *dirty; const osg_panel **drawing_panel; bool touch_capable; } osg_network_ui;
void osg_network_start(osg_network_ui);
void osg_network_touch(bool valid, bool down, int x, int y);
void osg_network_render(const osg_panel *, bool finished, bool success);
#endif
