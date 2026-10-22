# Download only the exact archive recorded in third_party/lvgl.lock.json.
# The same source/configuration is used by the native sanitizer harness.
idf_build_get_property(OSG_IDF_PYTHON PYTHON)
execute_process(
  COMMAND "${OSG_IDF_PYTHON}" "${CMAKE_CURRENT_LIST_DIR}/../../tools/lvgl_source.py"
  RESULT_VARIABLE OSG_LVGL_FETCH_RESULT OUTPUT_VARIABLE OSG_LVGL_SOURCE
  ERROR_VARIABLE OSG_LVGL_FETCH_ERROR OUTPUT_STRIP_TRAILING_WHITESPACE)
if(NOT OSG_LVGL_FETCH_RESULT EQUAL 0)
  message(FATAL_ERROR "Locked LVGL source unavailable: ${OSG_LVGL_FETCH_ERROR}")
endif()
file(GLOB_RECURSE OSG_LVGL_SOURCES "${OSG_LVGL_SOURCE}/src/*.c")
idf_component_register(
  SRCS ${OSG_LVGL_SOURCES} "../../render/lvgl_pool.c"
  INCLUDE_DIRS "${OSG_LVGL_SOURCE}" "${OSG_LVGL_SOURCE}/src" "../../render"
  REQUIRES esp_timer heap)
target_compile_definitions(${COMPONENT_LIB} PUBLIC LV_CONF_INCLUDE_SIMPLE LV_KCONFIG_IGNORE)
