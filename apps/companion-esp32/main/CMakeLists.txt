set(OSG_BOARD_SOURCE "${CMAKE_CURRENT_LIST_DIR}/../boards/headless.c" CACHE FILEPATH "Board adapter implementing osg_board_init")
set(OSG_PANEL_INCLUDE "${CMAKE_CURRENT_LIST_DIR}/../test" CACHE PATH "Directory containing generated panel.h")
idf_component_register(
  SRCS app_main.c ../core/companion.c ../core/raster.c ../boards/esp_lcd_bridge.c "${OSG_BOARD_SOURCE}"
       ../network/input.c ../network/json.c ../network/publication.c ../network/settings.c ../network/tls_transport.c ../network/wifi_store.c ../network/client.c ../third_party/cjson/cJSON.c
  INCLUDE_DIRS ../core ../boards ../network ../third_party/cjson "${OSG_PANEL_INCLUDE}"
  REQUIRES esp_lcd esp_timer esp_wifi esp_netif nvs_flash tcp_transport mbedtls bootloader_support esp_driver_gpio esp_driver_i2c esp_psram)
target_compile_options(${COMPONENT_LIB} PRIVATE -Wall -Wextra -Werror)

target_compile_definitions(${COMPONENT_LIB} PRIVATE CJSON_NESTING_LIMIT=24 CJSON_CIRCULAR_LIMIT=2048)
