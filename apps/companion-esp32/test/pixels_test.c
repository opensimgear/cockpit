#include "../network/pixels.c"
#include <assert.h>
#include <stdio.h>

static unsigned char flash[MAX_SECTORS * SECTOR_BYTES];
static esp_partition_t part = {sizeof(flash)};
static bool available = true, fail_read, fail_erase, fail_map, corrupt_write;
static int write_fail_after = -1;
static unsigned erased, writes, maps, unmapped, live_maps;
const esp_partition_t *esp_partition_find_first(int type, int subtype, const char *label) {
    assert(type == 1 && subtype == 0x40 && !strcmp(label, "images")); return available ? &part : NULL;
}
int esp_partition_read(const esp_partition_t *p, size_t offset, void *out, size_t n) {
    assert(p == &part && offset <= p->size && n <= p->size - offset);
    if (fail_read) return -1;
    memcpy(out, flash + offset, n); return 0;
}
int esp_partition_write(const esp_partition_t *p, size_t offset, const void *in, size_t n) {
    assert(p == &part && offset <= p->size && n <= p->size - offset);
    const unsigned char *data = in;
    bool torn = write_fail_after == 0;
    if (write_fail_after > 0) --write_fail_after;
    size_t limit = torn ? n / 2 : n;
    for (size_t i = 0; i < limit; ++i) {
        assert((flash[offset + i] & data[i]) == data[i]);
        flash[offset + i] &= data[i];
    }
    if (corrupt_write && limit) flash[offset + limit - 1] &= 0x7f;
    ++writes; return torn ? -1 : 0;
}
int esp_partition_erase_range(const esp_partition_t *p, size_t offset, size_t n) {
    assert(p == &part && offset <= p->size && n <= p->size - offset && offset % SECTOR_BYTES == 0 && n % SECTOR_BYTES == 0);
    if (fail_erase) return -1;
    for (unsigned i = 0; i < MAX_ASSETS; ++i) if (assets[i].present && assets[i].pins) {
        const size_t start = (size_t)(assets[i].sector + 1u) * SECTOR_BYTES;
        const size_t end = (size_t)(assets[i].sector + assets[i].sectors) * SECTOR_BYTES;
        assert(offset + n <= start || offset >= end); /* Never erase pinned pixel sectors. */
    }
    memset(flash + offset, 255, n); ++erased; return 0;
}
int esp_partition_mmap(const esp_partition_t *p, size_t offset, size_t n, int mode, const void **out, esp_partition_mmap_handle_t *handle) {
    assert(p == &part && offset == 0 && n == p->size && mode == 0);
    if (fail_map) return -1;
    *out = flash; *handle = 1; ++maps; ++live_maps; assert(live_maps == 1); return 0;
}
void esp_partition_munmap(esp_partition_mmap_handle_t handle) {
    assert(handle == 1 && live_maps == 1); --live_maps; ++unmapped;
}
static void reboot(void) {
    if (initialized) esp_partition_munmap(cache_mapping);
    initialized = false; cache_partition = NULL; cache_address = NULL;
    memset(assets, 0, sizeof(assets)); sector_count = 0; use_sequence = 0;
    available = true; fail_read = fail_erase = fail_map = corrupt_write = false; write_fail_after = -1;
}
static void fresh(unsigned sectors) {
    reboot(); assert(sectors <= MAX_SECTORS); part.size = sectors * SECTOR_BYTES;
    memset(flash, 255, sizeof(flash)); erased = writes = 0;
}
static void hash_for(unsigned id, char hash[72]) {
    assert(snprintf(hash, 72, "sha256:%064x", id) == 71);
}
static bool acquire(unsigned owner, unsigned id, unsigned w, unsigned h, unsigned *asset, bool *cached) {
    char hash[72]; hash_for(id, hash);
    return osg_pixels_acquire(owner, hash, w, h, (size_t)w * h * 3u, asset, cached);
}
static bool contains(unsigned id, unsigned w, unsigned h) {
    char hash[72]; hash_for(id, hash);
    return osg_pixels_contains(hash, w, h, (size_t)w * h * 3u);
}
static const unsigned char *create(unsigned owner, unsigned id, unsigned w, unsigned h, unsigned char value) {
    unsigned asset; bool cached = true; assert(acquire(owner, id, w, h, &asset, &cached) && !cached);
    unsigned char chunk[512]; memset(chunk, value, sizeof(chunk));
    for (size_t offset = 0, bytes = (size_t)w * h * 3u; offset < bytes;) {
        size_t count = bytes - offset; if (count > sizeof(chunk)) count = sizeof(chunk);
        assert(osg_pixels_write(asset, offset, chunk, count)); offset += count;
    }
    const unsigned char *pixels = osg_pixels_finish(asset); assert(pixels && pixels[0] == value);
    return pixels;
}
static void cached(unsigned owner, unsigned id, unsigned w, unsigned h, const unsigned char *expected) {
    unsigned asset; bool hit = false; unsigned before = erased;
    assert(acquire(owner, id, w, h, &asset, &hit) && hit);
    assert(osg_pixels_finish(asset) == expected && erased == before);
}
static void test_shared_persistent_cache(void) {
    fresh(MAX_SECTORS); assert(osg_pixels_panel_begin(0));
    const unsigned char *a = create(0, 1, 220, 120, 0x89);
    unsigned map_count = maps, before = erased;
    cached(1, 1, 220, 120, a); /* A different panel shares the same exact pixels. */
    assert(maps == map_count && erased == before);
    osg_pixels_panel_release(0); cached(1, 1, 220, 120, a);
    assert(!contains(1, 120, 220)); /* Rendered dimensions form part of the key. */
    const unsigned char *b = create(0, 1, 120, 220, 0x67); assert(a != b);
    assert(a[0] == 0x89 && b[0] == 0x67);
    reboot(); assert(osg_pixels_panel_begin(0)); assert(contains(1, 220, 120));
    cached(0, 1, 220, 120, a); cached(1, 1, 120, 220, b);
    assert(maps == map_count + 1);
}
static void test_lru_and_pinning(void) {
    fresh(6); create(0, 1, 1, 1, 1); osg_pixels_panel_release(0);
    create(0, 2, 1, 1, 2); osg_pixels_panel_release(0);
    create(0, 3, 1, 1, 3); osg_pixels_panel_release(0);
    unsigned asset; bool hit;
    assert(acquire(0, 1, 1, 1, &asset, &hit) && hit); osg_pixels_panel_release(0);
    reboot(); assert(osg_pixels_panel_begin(0));
    create(0, 4, 1, 1, 4);
    assert(contains(1, 1, 1) && !contains(2, 1, 1) && contains(3, 1, 1)); /* Persistent LRU, not FIFO. */
    fresh(4); const unsigned char *active = create(0, 1, 1, 1, 0x55);
    const unsigned char *candidate = create(1, 2, 1, 1, 0x66);
    unsigned before = erased;
    assert(!acquire(1, 3, 1, 1, &asset, &hit)); assert(!strcmp(osg_pixels_error(), "cache_full"));
    assert(active[0] == 0x55 && candidate[0] == 0x66 && erased == before);
    osg_pixels_panel_release(1); create(1, 3, 1, 1, 0x77);
    assert(active[0] == 0x55 && contains(1, 1, 1) && !contains(2, 1, 1));
    cached(1, 1, 1, 1, active); osg_pixels_panel_release(0);
    assert(!acquire(0, 4, 1, 1, &asset, &hit)); /* Shared active data stays pinned by the other owner. */
    osg_pixels_panel_release(1); create(0, 4, 1, 1, 4);
}
static void test_fragmentation_and_limits(void) {
    fresh(16);
    for (unsigned id = 1; id <= 8; ++id) { create(0, id, 1, 1, (unsigned char)id); osg_pixels_panel_release(0); }
    unsigned asset; bool hit;
    assert(acquire(0, 2, 1, 1, &asset, &hit)); const unsigned char *a = osg_pixels_finish(asset);
    assert(acquire(0, 6, 1, 1, &asset, &hit)); const unsigned char *b = osg_pixels_finish(asset);
    const unsigned char *large = create(1, 9, 64, 48, 9); /* Four contiguous sectors require more than one LRU victim. */
    assert(large && a[0] == 2 && b[0] == 6 && contains(2, 1, 1) && contains(6, 1, 1));
    assert(!contains(1, 1, 1) && !contains(3, 1, 1) && !contains(4, 1, 1));
    fresh(8);
    for (unsigned id = 1; id <= 4; ++id) { create(0, id, 1, 1, (unsigned char)id); osg_pixels_panel_release(0); }
    assert(acquire(0, 2, 1, 1, &asset, &hit)); a = osg_pixels_finish(asset);
    assert(acquire(0, 4, 1, 1, &asset, &hit)); b = osg_pixels_finish(asset);
    assert(!acquire(1, 5, 64, 48, &asset, &hit)); /* Enough total free sectors, but pinned holes prohibit a contiguous map. */
    assert(!strcmp(osg_pixels_error(), "cache_full") && a[0] == 2 && b[0] == 4);
    fresh(MAX_SECTORS);
    for (unsigned id = 1; id <= MAX_ASSETS; ++id) { create(0, id, 1, 1, 1); osg_pixels_panel_release(0); }
    create(0, MAX_ASSETS + 1, 1, 1, 1); assert(!contains(1, 1, 1) && contains(2, 1, 1));
    assert(live_maps == 1); /* No per-asset mappings or MMU leaks. */
}
static void test_interrupted_and_corrupt_entries(void) {
    fresh(12); unsigned asset; bool hit;
    assert(acquire(0, 1, 2, 1, &asset, &hit) && !hit);
    unsigned char data[] = {255, 255, 255, 255, 255, 255};
    assert(!osg_pixels_write(asset, 1, data, 3)); assert(osg_pixels_write(asset, 0, data, 3));
    assert(!osg_pixels_finish(asset)); reboot(); assert(!contains(1, 2, 1)); /* Missing payload. */
    assert(acquire(0, 1, 2, 1, &asset, &hit) && !hit);
    assert(osg_pixels_write(asset, 0, data, sizeof(data)));
    write_fail_after = 1; assert(!osg_pixels_finish(asset)); /* Torn commit marker. */
    reboot(); assert(!contains(1, 2, 1));
    const unsigned char *pixels = create(0, 1, 2, 1, 255); osg_pixels_panel_release(0);
    ((unsigned char *)pixels)[2] = 0;
    assert(acquire(1, 1, 2, 1, &asset, &hit) && !hit); /* CRC invalidation re-downloads unpinned pixels. */
    assert(osg_pixels_write(asset, 0, data, sizeof(data))); pixels = osg_pixels_finish(asset); assert(pixels);
    ((unsigned char *)pixels)[2] = 0;
    assert(!acquire(0, 1, 2, 1, &asset, &hit) && !strcmp(osg_pixels_error(), "cache_active_corrupt"));
    assert(pixels[2] == 0); /* A damaged pinned entry is never erased underneath the renderer. */
    fresh(12); assert(acquire(0, 1, 2, 1, &asset, &hit)); corrupt_write = true;
    assert(osg_pixels_write(asset, 0, data, sizeof(data))); corrupt_write = false;
    assert(!osg_pixels_finish(asset) && !strcmp(osg_pixels_error(), "cache_corrupt"));
    fresh(12); create(0, 1, 2, 1, 255); osg_pixels_panel_release(0);
    flash[16] ^= 1; reboot(); assert(!contains(1, 2, 1)); /* Metadata CRC. */
    fresh(12); memcpy(flash, "OSGI", 4); reboot(); assert(!contains(1, 2, 1));
    create(0, 1, 2, 1, 9); /* Legacy two-slot data is safely reclaimed. */
}
static void test_journal_and_failures(void) {
    fresh(8); const unsigned char *a = create(0, 1, 1, 1, 255); unsigned asset; bool hit;
    unsigned before = erased;
    for (unsigned i = 0; i < 20; ++i) cached(1, 1, 1, 1, a);
    assert(erased == before); /* Cache hits only append eight metadata bytes. */
    for (unsigned i = 0; i < (SECTOR_BYTES - HEADER_BYTES) / JOURNAL_BYTES; ++i)
        assert(acquire(1, 1, 1, 1, &asset, &hit) && hit);
    assert(erased == before + 1 && a[0] == 255);
    reboot(); cached(0, 1, 1, 1, a);
    write_fail_after = 0; assert(!acquire(1, 1, 1, 1, &asset, &hit)); /* Torn LRU record cannot hide committed pixels. */
    reboot(); cached(0, 1, 1, 1, a);
    fail_read = true; assert(!acquire(1, 1, 1, 1, &asset, &hit)); fail_read = false;
    fresh(8); available = false; assert(osg_pixels_panel_begin(0)); assert(!initialized);
    assert(!acquire(0, 1, 1, 1, &asset, &hit)); assert(!strcmp(osg_pixels_error(), "cache_unavailable"));
    available = true; fail_read = true; assert(!acquire(0, 1, 1, 1, &asset, &hit)); fail_read = false;
    fail_map = true; assert(!acquire(0, 1, 1, 1, &asset, &hit)); fail_map = false;
    fail_erase = true; assert(!acquire(0, 1, 1, 1, &asset, &hit)); fail_erase = false;
    assert(acquire(0, 1, 1, 1, &asset, &hit));
    unsigned char pixel[] = {255, 255, 255}; write_fail_after = 0;
    assert(!osg_pixels_write(asset, 0, pixel, 3)); reboot(); assert(!contains(1, 1, 1));
    assert(!osg_pixels_panel_begin(2)); assert(!osg_pixels_finish(MAX_ASSETS));
    char hash[72]; hash_for(1, hash);
    assert(!osg_pixels_acquire(0, "bad", 1, 1, 3, &asset, &hit));
    assert(!osg_pixels_acquire(2, hash, 1, 1, 3, &asset, &hit));
    assert(!osg_pixels_acquire(0, hash, 0, 1, 3, &asset, &hit));
    assert(!osg_pixels_acquire(0, hash, 801, 1, 2403, &asset, &hit));
    assert(!osg_pixels_acquire(0, hash, 1, 1, 4, &asset, &hit));
    assert(!osg_pixels_acquire(0, hash, 800, 800, 1920000, &asset, &hit));
    assert(!osg_pixels_acquire(0, hash, 1, 1, 3, NULL, &hit));
    assert(!osg_pixels_acquire(0, hash, 1, 1, 3, &asset, NULL));
    hash[8] = 'z'; assert(!osg_pixels_acquire(0, hash, 1, 1, 3, &asset, &hit));
    hash_for(1, hash); hash[0] = 'x'; assert(!osg_pixels_acquire(0, hash, 1, 1, 3, &asset, &hit));
    assert(!osg_pixels_write(MAX_ASSETS, 0, pixel, 3));
    assert(!osg_pixels_write(0, 0, NULL, 3)); assert(!osg_pixels_write(0, 0, pixel, 0));
    fresh(4); create(0, 1, 1, 1, 1); osg_pixels_panel_release(0);
    create(0, 2, 1, 1, 2); osg_pixels_panel_release(0);
    fail_erase = true; assert(!acquire(0, 3, 1, 1, &asset, &hit)); fail_erase = false;
    assert(contains(1, 1, 1) && contains(2, 1, 1));
    fresh(4); assert(acquire(0, 1, 1, 1, &asset, &hit));
    assert(osg_pixels_write(asset, 0, pixel, sizeof(pixel))); fail_read = true;
    assert(!osg_pixels_finish(asset)); fail_read = false; write_fail_after = 0;
    assert(!osg_pixels_finish(asset)); reboot(); assert(!contains(1, 1, 1));
    fresh(4); create(0, 1, 1, 1, 1); osg_pixels_panel_release(0);
    assert(!osg_pixels_finish(0));
    fresh(1); assert(!acquire(0, 1, 1, 1, &asset, &hit));
}
static void test_failed_metadata_renewal_never_makes_active_pixels_writable(void) {
    fresh(8); const unsigned char *pixels = create(0, 1, 1, 1, 0x55);
    reboot(); unsigned asset; bool hit;
    assert(acquire(0, 1, 1, 1, &asset, &hit) && hit);
    /* A rebooted entry has written == 0; this used to allow a failed metadata
     * renewal to turn its active pixels into the target of a new download. */
    assert(assets[asset].written == 0 && assets[asset].complete);
    while (assets[asset].journal + JOURNAL_BYTES <= SECTOR_BYTES)
        assert(acquire(1, 1, 1, 1, &asset, &hit) && hit);
    osg_pixels_panel_release(1);
    write_fail_after = 0;
    assert(!acquire(1, 1, 1, 1, &asset, &hit)); /* Torn renewed metadata header. */
    assert(assets[asset].pins == 1 && assets[asset].complete && !assets[asset].committed);
    unsigned char different[] = {0, 0, 0};
    assert(!osg_pixels_write(asset, 0, different, sizeof(different)));
    assert(pixels[0] == 0x55 && pixels[1] == 0x55 && pixels[2] == 0x55);
    assert(!acquire(1, 1, 1, 1, &asset, &hit)); /* Repeated metadata failure remains non-writable. */
    assert(!osg_pixels_write(asset, 0, different, sizeof(different)));
    write_fail_after = -1;
    assert(acquire(1, 1, 1, 1, &asset, &hit) && hit); /* Repair metadata, reuse immutable pixels. */
    assert(osg_pixels_finish(asset) == pixels && assets[asset].committed);
    assert(!osg_pixels_write(asset, 0, different, sizeof(different)));
    assert(pixels[0] == 0x55 && pixels[1] == 0x55 && pixels[2] == 0x55);
    reboot(); cached(0, 1, 1, 1, pixels);
}
int main(void) {
    test_shared_persistent_cache(); test_lru_and_pinning(); test_fragmentation_and_limits();
    test_interrupted_and_corrupt_entries(); test_journal_and_failures();
    test_failed_metadata_renewal_never_makes_active_pixels_writable();
    reboot(); assert(!live_maps && maps == unmapped);
    puts("Shared persistent image cache: CRC/commit recovery, dimension keys, LRU, pinning, fragmentation and bounded mapping passed");
    return 0;
}
