#include "pixel_cache.h"
#include "esp_partition.h"
#include <stdint.h>
#include <string.h>

/* Immutable assets occupy a metadata sector and contiguous pixel sectors.
 * Never relocate assets: the renderer holds mapped flash pointers. Metadata
 * is disposable; an interrupted write costs a miss, not an active panel. */
#define SECTOR_BYTES 4096u
#define MAX_SECTORS (0x260000u / SECTOR_BYTES)
#define MAX_ASSETS 128u
#define MAX_IMAGE_BYTES (1200u * 1024u)
#define FORMAT_RGB565_ALPHA 1u
#define HEADER_BYTES 80u
#define JOURNAL_BYTES 8u
#define COMMITTED 0x3253474fu

typedef struct {
    uint32_t bytes, crc, used, written, rolling;
    unsigned char hash[32];
    uint16_t sector, sectors, width, height, journal;
    unsigned char pins;
    /* Pixel immutability is independent from disposable metadata durability.
     * A failed journal/header renewal must never make displayed pixels writable. */
    bool present, complete, committed;
} cache_asset;
static cache_asset assets[MAX_ASSETS];
static const esp_partition_t *cache_partition;
static const unsigned char *cache_address;
static esp_partition_mmap_handle_t cache_mapping;
static uint32_t use_sequence;
static unsigned sector_count;
static bool initialized;
static const char *last_error = "cache_invalid";

const char *osg_pixels_error(void) { return last_error; }
static bool fail(const char *reason) { last_error = reason; return false; }
static uint32_t get32(const unsigned char *p) {
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}
static void put32(unsigned char *p, uint32_t n) {
    p[0] = (unsigned char)n; p[1] = (unsigned char)(n >> 8);
    p[2] = (unsigned char)(n >> 16); p[3] = (unsigned char)(n >> 24);
}
static uint32_t crc_update(uint32_t crc, const void *data, size_t bytes) {
    const unsigned char *p = data;
    while (bytes--) {
        crc ^= *p++;
        for (unsigned bit = 0; bit < 8; ++bit)
            crc = (crc >> 1) ^ (0xedb88320u & (0u - (crc & 1u)));
    }
    return crc;
}
static bool newer(uint32_t a, uint32_t b) { return (int32_t)(a - b) > 0; }
static bool read_at(size_t offset, void *data, size_t bytes) {
    return esp_partition_read(cache_partition, offset, data, bytes) == ESP_OK;
}
static bool decode_hash(const char *text, unsigned char hash[32]) {
    if (!text || strlen(text) != 71 || memcmp(text, "sha256:", 7)) return false;
    for (unsigned i = 0; i < 32; ++i) {
        unsigned n = 0;
        for (unsigned j = 0; j < 2; ++j) {
            unsigned char c = (unsigned char)text[7 + 2 * i + j];
            unsigned value;
            if (c >= '0' && c <= '9') value = c - '0';
            else if (c >= 'a' && c <= 'f') value = c - 'a' + 10u;
            else return false;
            n = n * 16 + value;
        }
        hash[i] = (unsigned char)n;
    }
    return true;
}
static bool image_crc(const cache_asset *a, uint32_t *result) {
    unsigned char chunk[512];
    uint32_t crc = UINT32_MAX;
    size_t offset = (size_t)(a->sector + 1u) * SECTOR_BYTES;
    for (size_t count = 0; count < a->bytes;) {
        size_t n = a->bytes - count;
        if (n > sizeof(chunk)) n = sizeof(chunk);
        if (!read_at(offset + count, chunk, n)) return false;
        crc = crc_update(crc, chunk, n); count += n;
    }
    *result = ~crc;
    return true;
}
static bool header_valid(const unsigned char *h, unsigned sector, cache_asset *a) {
    if (memcmp(h, "OSG2", 4) || get32(h + 4) != 1 || get32(h + 8) != SECTOR_BYTES ||
        get32(h + 28) != FORMAT_RGB565_ALPHA || get32(h + 72) != COMMITTED ||
        get32(h + 68) != ~crc_update(UINT32_MAX, h, 68)) return false;
    const uint32_t bytes = get32(h + 12), width = get32(h + 16), height = get32(h + 20);
    const uint32_t sectors = get32(h + 24);
    if (!width || width > 800 || !height || height > 800 || !bytes || bytes > MAX_IMAGE_BYTES ||
        bytes != width * height * 3u || sectors != 1u + (bytes + SECTOR_BYTES - 1u) / SECTOR_BYTES ||
        sectors > sector_count - sector) return false;
    memset(a, 0, sizeof(*a));
    a->bytes = bytes; a->width = (uint16_t)width; a->height = (uint16_t)height;
    a->sector = (uint16_t)sector; a->sectors = (uint16_t)sectors; a->crc = get32(h + 64);
    memcpy(a->hash, h + 32, sizeof(a->hash)); a->present = a->complete = a->committed = true;
    return true;
}
static bool initialize(void) {
    if (initialized) return true;
    cache_partition = esp_partition_find_first(ESP_PARTITION_TYPE_DATA, 0x40, "images");
    if (!cache_partition || cache_partition->size < 2u * SECTOR_BYTES ||
        cache_partition->size > MAX_SECTORS * SECTOR_BYTES || cache_partition->size % SECTOR_BYTES)
        return fail("cache_unavailable");
    sector_count = (unsigned)(cache_partition->size / SECTOR_BYTES);
    memset(assets, 0, sizeof(assets)); use_sequence = 0;
    unsigned index = 0;
    for (unsigned sector = 0; sector < sector_count;) {
        unsigned char h[HEADER_BYTES]; cache_asset candidate;
        if (!read_at((size_t)sector * SECTOR_BYTES, h, sizeof(h))) return fail("cache_io");
        if (!header_valid(h, sector, &candidate)) { ++sector; continue; }
        unsigned char record[JOURNAL_BYTES];
        for (unsigned at = HEADER_BYTES; at + JOURNAL_BYTES <= SECTOR_BYTES; at += JOURNAL_BYTES) {
            if (!read_at((size_t)sector * SECTOR_BYTES + at, record, sizeof(record))) return fail("cache_io");
            const uint32_t sequence = get32(record), inverse = get32(record + 4);
            if (sequence == UINT32_MAX && inverse == UINT32_MAX) { candidate.journal = (uint16_t)at; break; }
            if (inverse == ~sequence && (!candidate.used || newer(sequence, candidate.used))) candidate.used = sequence;
            candidate.journal = (uint16_t)(at + JOURNAL_BYTES);
        }
        if (newer(candidate.used, use_sequence)) use_sequence = candidate.used;
        if (index < MAX_ASSETS) assets[index++] = candidate;
        sector += candidate.sectors;
    }
    /* One mapping bounds MMU use even with tiny assets (38 pages on Sunton).
     * SDK partition writes/erases invalidate the corresponding flash cache. */
    const void *address = NULL;
    if (esp_partition_mmap(cache_partition, 0, cache_partition->size, ESP_PARTITION_MMAP_DATA,
                           &address, &cache_mapping) != ESP_OK) return fail("cache_unavailable");
    cache_address = address; initialized = true;
    return true;
}
static void make_header(const cache_asset *a, unsigned char h[HEADER_BYTES]) {
    memset(h, 255, HEADER_BYTES); memcpy(h, "OSG2", 4);
    put32(h + 4, 1); put32(h + 8, SECTOR_BYTES); put32(h + 12, a->bytes);
    put32(h + 16, a->width); put32(h + 20, a->height); put32(h + 24, a->sectors);
    put32(h + 28, FORMAT_RGB565_ALPHA); memcpy(h + 32, a->hash, sizeof(a->hash));
    put32(h + 64, a->crc); put32(h + 68, ~crc_update(UINT32_MAX, h, 68));
}
static bool commit_header(cache_asset *a) {
    unsigned char h[HEADER_BYTES], marker[4]; make_header(a, h); put32(marker, COMMITTED);
    const size_t base = (size_t)a->sector * SECTOR_BYTES;
    /* Commit is separate and last; torn headers are never eligible for reuse. */
    if (esp_partition_write(cache_partition, base, h, sizeof(h)) != ESP_OK ||
        esp_partition_write(cache_partition, base + 72, marker, sizeof(marker)) != ESP_OK)
        return fail("cache_io");
    a->committed = true; a->journal = HEADER_BYTES;
    return true;
}
static bool touch(cache_asset *a) {
    unsigned char record[JOURNAL_BYTES];
    if (++use_sequence == 0) ++use_sequence;
    a->used = use_sequence;
    if (!a->complete) return true;
    if (!a->committed || a->journal + JOURNAL_BYTES > SECTOR_BYTES) {
        /* Renew only metadata; active mapped pixels never move. Reset here
         * loses a reusable record, while the current panel remains readable. */
        if (esp_partition_erase_range(cache_partition, (size_t)a->sector * SECTOR_BYTES,
                                      SECTOR_BYTES) != ESP_OK) return fail("cache_io");
        a->committed = false;
        if (!commit_header(a)) return false;
    }
    put32(record, a->used); put32(record + 4, ~a->used);
    const size_t offset = (size_t)a->sector * SECTOR_BYTES + a->journal;
    /* Consume torn cells rather than trying to set already programmed bits. */
    a->journal += JOURNAL_BYTES;
    return esp_partition_write(cache_partition, offset, record, sizeof(record)) == ESP_OK || fail("cache_io");
}
void osg_pixels_panel_release(unsigned owner) {
    if (owner > 1) return;
    for (unsigned i = 0; i < MAX_ASSETS; ++i) {
        assets[i].pins &= (unsigned char)~(1u << owner);
        if (!assets[i].pins && !assets[i].committed) assets[i].present = false;
    }
}
bool osg_pixels_panel_begin(unsigned owner) {
    if (owner > 1) return fail("cache_invalid");
    /* Image-free panels must work without reserving flash MMU pages. */
    osg_pixels_panel_release(owner);
    return true;
}
static bool free_run(unsigned sectors, unsigned *start) {
    unsigned char occupied[MAX_SECTORS] = {0};
    for (unsigned i = 0; i < MAX_ASSETS; ++i) if (assets[i].present)
        memset(occupied + assets[i].sector, 1, assets[i].sectors);
    unsigned run = 0;
    for (unsigned i = 0; i < sector_count; ++i) {
        run = occupied[i] ? 0 : run + 1;
        if (run == sectors) { *start = i + 1 - sectors; return true; }
    }
    return false;
}
static int empty_asset(void) {
    for (unsigned i = 0; i < MAX_ASSETS; ++i) if (!assets[i].present) return (int)i;
    return -1;
}
static bool evict_oldest(void) {
    int oldest = -1;
    for (unsigned i = 0; i < MAX_ASSETS; ++i)
        if (assets[i].present && !assets[i].pins &&
            (oldest < 0 || newer(assets[oldest].used, assets[i].used))) oldest = (int)i;
    if (oldest < 0) return fail("cache_full");
    cache_asset *a = &assets[oldest];
    /* Invalidate the persistent header before the allocator may reuse data. */
    if (esp_partition_erase_range(cache_partition, (size_t)a->sector * SECTOR_BYTES,
                                  SECTOR_BYTES) != ESP_OK) return fail("cache_io");
    memset(a, 0, sizeof(*a));
    return true;
}
bool osg_pixels_contains(const char *text, unsigned width, unsigned height, size_t bytes) {
    unsigned char hash[32];
    if (!decode_hash(text, hash) || !initialize()) return false;
    for (unsigned i = 0; i < MAX_ASSETS; ++i)
        if (assets[i].present && assets[i].committed && assets[i].width == width &&
            assets[i].height == height && assets[i].bytes == bytes &&
            !memcmp(assets[i].hash, hash, sizeof(hash))) return true;
    return false;
}
bool osg_pixels_acquire(unsigned owner, const char *text, unsigned width, unsigned height,
                        size_t bytes, unsigned *asset, bool *cached) {
    unsigned char hash[32];
    if (owner > 1 || !asset || !cached || !decode_hash(text, hash) || !width || width > 800 ||
        !height || height > 800 || !bytes || bytes > MAX_IMAGE_BYTES || bytes != (size_t)width * height * 3u)
        return fail("cache_invalid");
    if (!initialize()) return false;
    for (unsigned i = 0; i < MAX_ASSETS; ++i) {
        cache_asset *a = &assets[i];
        if (!a->present || a->width != width || a->height != height || a->bytes != bytes ||
            memcmp(a->hash, hash, sizeof(hash))) continue;
        if (a->complete) {
            uint32_t crc;
            if (!image_crc(a, &crc)) return fail("cache_io");
            if (crc != a->crc) {
                if (a->pins) return fail("cache_active_corrupt");
                if (esp_partition_erase_range(cache_partition, (size_t)a->sector * SECTOR_BYTES,
                                              SECTOR_BYTES) != ESP_OK) return fail("cache_io");
                memset(a, 0, sizeof(*a)); break;
            }
        }
        if (!touch(a)) return false;
        a->pins |= (unsigned char)(1u << owner); *asset = i; *cached = a->complete;
        return true;
    }
    const unsigned sectors = 1u + (unsigned)((bytes + SECTOR_BYTES - 1u) / SECTOR_BYTES);
    unsigned start; int index;
    while ((index = empty_asset()) < 0 || !free_run(sectors, &start)) if (!evict_oldest()) return false;
    if (esp_partition_erase_range(cache_partition, (size_t)start * SECTOR_BYTES,
                                  (size_t)sectors * SECTOR_BYTES) != ESP_OK) return fail("cache_io");
    cache_asset *a = &assets[index]; memset(a, 0, sizeof(*a));
    a->present = true; a->sector = (uint16_t)start; a->sectors = (uint16_t)sectors;
    a->width = (uint16_t)width; a->height = (uint16_t)height; a->bytes = (uint32_t)bytes;
    a->rolling = UINT32_MAX; a->pins = (unsigned char)(1u << owner); memcpy(a->hash, hash, sizeof(hash));
    (void)touch(a); *asset = (unsigned)index; *cached = false;
    return true;
}
bool osg_pixels_write(unsigned asset, size_t offset, const void *bytes, size_t count) {
    if (!initialized || asset >= MAX_ASSETS || !bytes || !count) return fail("cache_invalid");
    cache_asset *a = &assets[asset];
    if (!a->present || !a->pins || a->complete || offset != a->written || count > a->bytes - a->written)
        return fail("cache_invalid");
    if (esp_partition_write(cache_partition, (size_t)(a->sector + 1u) * SECTOR_BYTES + offset,
                             bytes, count) != ESP_OK) return fail("cache_io");
    a->rolling = crc_update(a->rolling, bytes, count); a->written += (uint32_t)count;
    return true;
}
const unsigned char *osg_pixels_finish(unsigned asset) {
    if (!initialized || asset >= MAX_ASSETS) { fail("cache_invalid"); return NULL; }
    cache_asset *a = &assets[asset];
    if (!a->present || !a->pins) { fail("cache_invalid"); return NULL; }
    if (!a->complete) {
        uint32_t crc;
        if (a->written != a->bytes) { fail("cache_invalid"); return NULL; }
        if (!image_crc(a, &crc)) { fail("cache_io"); return NULL; }
        if (crc != ~a->rolling) { fail("cache_corrupt"); return NULL; }
        a->crc = crc; a->complete = true;
        if (!commit_header(a) || !touch(a)) return NULL;
    }
    else if (!a->committed && !touch(a)) return NULL;
    return cache_address + (size_t)(a->sector + 1u) * SECTOR_BYTES;
}
