set(OSG_BOARD_SOURCE "${CMAKE_CURRENT_LIST_DIR}/../boards/headless.c" CACHE FILEPATH "Board adapter implementing osg_board_init")
set(OSG_PANEL_INCLUDE "${CMAKE_CURRENT_LIST_DIR}/../test" CACHE PATH "Directory containing generated panel.h")
idf_component_register(
  SRCS app_main.c ../core/companion.c ../core/widget.c ../core/raster.c ../render/lvgl_backend.c ../render/widget_fonts.c ../render/widget_source.c ../render/widget_state.c ../render/widget_draw.c ../render/package.c ../render/content_store.c ../boards/esp_lcd_bridge.c "${OSG_BOARD_SOURCE}"
       ../network/serial_wire.c ../network/serial_json.c ../network/serial.c ../network/input.c ../network/json.c ../network/publication.c ../network/panel_source.c ../network/settings.c ../network/tls_transport.c ../network/wifi_store.c ../network/storage.c ../network/client.c ../network/pixels.c ../third_party/cjson/cJSON.c
  INCLUDE_DIRS ../core ../boards ../network ../render ../third_party/cjson "${OSG_PANEL_INCLUDE}"
  REQUIRES osg_lvgl esp_lcd esp_timer esp_wifi esp_netif nvs_flash tcp_transport mbedtls bootloader_support esp_driver_gpio esp_driver_spi esp_driver_i2c esp_driver_uart esp_psram
  EMBED_FILES ../../../packages/companion-ui/assets/fonts/osg-sans.ttf ../../../packages/companion-ui/assets/fonts/osg-mono.ttf
              ../../../packages/companion-ui/assets/fonts/OFL-Rajdhani.txt ../../../packages/companion-ui/assets/fonts/OFL-IBMPlexMono.txt)
target_compile_options(${COMPONENT_LIB} PRIVATE -Wall -Wextra -Werror)

target_compile_definitions(${COMPONENT_LIB} PRIVATE CJSON_NESTING_LIMIT=24 CJSON_CIRCULAR_LIMIT=2048)

target_compile_definitions(${COMPONENT_LIB} PRIVATE OSG_BOARD_PROFILE="${OSG_BOARD_PROFILE}")
