#ifndef OSG_CONTENT_STORE_ESP_H
#define OSG_CONTENT_STORE_ESP_H
#include "content_store.h"
#include "esp_partition.h"
#include "mbedtls/sha256.h"
typedef struct { const esp_partition_t *partition; mbedtls_sha256_context hash; bool initialized; } osg_content_esp;
/* Call once per adapter lifetime. Missing/legacy partition tables return
 * NOT_FOUND; mismatched/encrypted partitions fail without erase or write. */
osg_content_status osg_content_esp_open(osg_content_esp *, osg_content_store *, const char *board_profile,
    void *validation_context,
    bool (*validate_manifest)(void *, const osg_content_view *, const osg_content_candidate *),
    bool (*validate_content)(void *, const osg_content_view *, const osg_content_candidate *));
/* Stop all users of the associated store before closing. */
void osg_content_esp_close(osg_content_esp *);
#endif
