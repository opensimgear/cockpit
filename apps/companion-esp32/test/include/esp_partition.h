#pragma once
#include <stddef.h>
#include <stdint.h>
#define ESP_OK 0
#define ESP_PARTITION_TYPE_DATA 1
#define ESP_PARTITION_MMAP_DATA 0
typedef unsigned esp_partition_mmap_handle_t;
typedef struct {unsigned size;} esp_partition_t;
const esp_partition_t *esp_partition_find_first(int,int,const char *);
int esp_partition_read(const esp_partition_t *,size_t,void *,size_t);
int esp_partition_write(const esp_partition_t *,size_t,const void *,size_t);
int esp_partition_erase_range(const esp_partition_t *,size_t,size_t);
int esp_partition_mmap(const esp_partition_t *,size_t,size_t,int,const void **,esp_partition_mmap_handle_t *);
void esp_partition_munmap(esp_partition_mmap_handle_t);
