#ifndef OSG_CONTENT_STORE_H
#define OSG_CONTENT_STORE_H
#include "package.h"
#define OSG_CONTENT_CHUNK_BYTES 2048u
#define OSG_CONTENT_HEADER_BYTES 512u
#define OSG_CONTENT_MANIFEST OSG_RENDER_OBJECT_KINDS
#define OSG_CONTENT_PARTS (OSG_RENDER_OBJECT_KINDS + 1u)

typedef enum { OSG_CONTENT_OK, OSG_CONTENT_ARGUMENT, OSG_CONTENT_IO, OSG_CONTENT_BUDGET, OSG_CONTENT_BUSY,
    OSG_CONTENT_ORDER, OSG_CONTENT_HASH, OSG_CONTENT_INVALID, OSG_CONTENT_STATE, OSG_CONTENT_NOT_FOUND, OSG_CONTENT_EXHAUSTED } osg_content_status;
typedef struct { uint32_t bytes; uint8_t sha256[32]; } osg_content_object;
typedef struct {
    uint32_t manifest_bytes; uint16_t renderer_abi;
    uint8_t package_sha256[32], source_sha256[32], profile_sha256[32], semantic_sha256[32];
    osg_content_object objects[OSG_RENDER_OBJECT_KINDS];
} osg_content_candidate;
struct osg_content_store;
typedef struct {
    struct osg_content_store *store; uint64_t generation; unsigned slot; bool held;
} osg_content_view;
/* Raw callbacks must synchronously consume buffers. NOR write may only clear
 * bits; erase operates on complete erase_bytes sectors. No credentials/NVS are
 * accessed. The caller serializes state operations; pinned reads may run while
 * another slot is staged. Callbacks must not reenter this store. */
typedef struct {
    void *context; uint32_t bytes, erase_bytes, manifest_limit, package_limit;
    bool (*read)(void *, uint32_t, void *, size_t);
    bool (*write)(void *, uint32_t, const void *, size_t);
    bool (*erase)(void *, uint32_t, uint32_t);
    void *hash_context;
    bool (*hash_begin)(void *); bool (*hash_update)(void *, const void *, size_t); bool (*hash_finish)(void *, uint8_t[32]);
    void *validation_context;
    /* Mandatory: compare exact cached manifest bytes with ALL candidate fields,
     * object identities/lengths/hashes and target/source metadata. */
    bool (*validate_manifest)(void *, const osg_content_view *, const osg_content_candidate *);
    /* Mandatory: validate object structure, references/semantics and per-profile
     * budgets. Neither callback opens or arms an input session. */
    bool (*validate_content)(void *, const osg_content_view *, const osg_content_candidate *);
} osg_content_io;
typedef struct {
    osg_content_candidate candidate; uint64_t generation; uint32_t offsets[OSG_CONTENT_PARTS], payload_bytes;
    unsigned pins; bool valid;
} osg_content_slot;
typedef struct osg_content_store {
    osg_content_io io; osg_content_slot slots[2]; uint32_t slot_bytes, slot_offsets[2];
    int active_slot, active_journal, stage_slot, validating_slot;
    uint32_t erase_progress, written[OSG_CONTENT_PARTS]; bool ready, verified;
    uint8_t scratch[OSG_CONTENT_CHUNK_BYTES];
} osg_content_store;

osg_content_status osg_content_init(osg_content_store *, osg_content_io);
/* Revalidate journal, exact manifest and every object with bounded reads. A
 * restored package remains an offline content view; this never restores input. */
osg_content_status osg_content_scan(osg_content_store *);
osg_content_status osg_content_begin(osg_content_store *, const osg_content_candidate *);
/* One erase sector per call. done becomes true after immutable header write. */
osg_content_status osg_content_erase_step(osg_content_store *, bool *done);
osg_content_status osg_content_write(osg_content_store *, unsigned part, uint32_t offset, const void *, size_t);
osg_content_status osg_content_verify(osg_content_store *);
osg_content_status osg_content_commit(osg_content_store *);
void osg_content_abort(osg_content_store *);
/* A view has one owner and must be released exactly once; do not copy ownership.
 * Pinning prevents re-erasing an old scene still being consumed after a swap. */
osg_content_status osg_content_acquire(osg_content_store *, osg_content_view *);
void osg_content_release(osg_content_view *);
const osg_content_candidate *osg_content_metadata(const osg_content_view *);
osg_content_status osg_content_read(const osg_content_view *, unsigned part, uint32_t, void *, size_t);
typedef struct { const osg_content_view *view; unsigned part; } osg_content_reader;
/* osg_render_read-compatible context for package/scene decoders. */
bool osg_content_object_read(void *, uint32_t, void *, size_t);
#endif
