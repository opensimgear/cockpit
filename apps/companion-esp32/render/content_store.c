#include "content_store.h"
#include <limits.h>
#include <string.h>
#define JOURNAL_BYTES 128u
#define COMMIT_MARKER UINT32_C(0x434f4d54)
static const uint8_t slot_magic[8]={'O','S','G','C','O','N','T',0};
static uint16_t u16(const uint8_t *p){return (uint16_t)(p[0]|(uint16_t)p[1]<<8);}
static uint32_t u32(const uint8_t *p){return p[0]|(uint32_t)p[1]<<8|(uint32_t)p[2]<<16|(uint32_t)p[3]<<24;}
static uint64_t u64(const uint8_t *p){return u32(p)|(uint64_t)u32(p+4)<<32;}
static void p16(uint8_t *p,uint16_t n){p[0]=(uint8_t)n;p[1]=(uint8_t)(n>>8);}
static void p32(uint8_t *p,uint32_t n){for(unsigned i=0;i<4;i++)p[i]=(uint8_t)(n>>(i*8));}
static void p64(uint8_t *p,uint64_t n){p32(p,(uint32_t)n);p32(p+4,(uint32_t)(n>>32));}
static bool zeros(const uint8_t *p,size_t n){for(size_t i=0;i<n;i++)if(p[i])return false;return true;}
static bool equal(const uint8_t a[32],const uint8_t b[32]){unsigned difference=0;for(unsigned i=0;i<32;i++)difference|=a[i]^b[i];return difference==0;}
static uint32_t part_bytes(const osg_content_slot *slot,unsigned part){return part==OSG_CONTENT_MANIFEST?slot->candidate.manifest_bytes:slot->candidate.objects[part].bytes;}
static osg_content_status layout(osg_content_store *s,osg_content_slot *slot){
    const osg_content_candidate *c=&slot->candidate;
    if(c->renderer_abi!=1||!c->manifest_bytes||c->manifest_bytes>s->io.manifest_limit||c->objects[0].bytes<32||c->objects[1].bytes<32)return OSG_CONTENT_INVALID;
    if(!equal(c->semantic_sha256,c->objects[0].sha256))return OSG_CONTENT_INVALID;
    uint64_t used=((uint64_t)c->manifest_bytes+3)&~UINT64_C(3),raw=c->manifest_bytes;slot->offsets[OSG_CONTENT_MANIFEST]=0;
    for(unsigned i=0;i<OSG_RENDER_OBJECT_KINDS;i++){
        const osg_content_object *object=&c->objects[i];
        if(!object->bytes){if(!zeros(object->sha256,32))return OSG_CONTENT_INVALID;slot->offsets[i]=0;continue;}
        if(used>UINT32_MAX)return OSG_CONTENT_BUDGET;
        slot->offsets[i]=(uint32_t)used;used+=((uint64_t)object->bytes+3)&~UINT64_C(3);raw+=object->bytes;
    }
    if(raw>s->io.package_limit||used>s->slot_bytes-s->io.erase_bytes)return OSG_CONTENT_BUDGET;
    slot->payload_bytes=(uint32_t)used;return OSG_CONTENT_OK;
}
static void header_encode(const osg_content_slot *slot,uint8_t out[OSG_CONTENT_HEADER_BYTES]){
    const osg_content_candidate *c=&slot->candidate;memset(out,0,OSG_CONTENT_HEADER_BYTES);memcpy(out,slot_magic,8);
    p16(out+8,1);p16(out+10,OSG_CONTENT_HEADER_BYTES);p64(out+12,slot->generation);p32(out+20,slot->payload_bytes);p32(out+24,c->manifest_bytes);p16(out+28,c->renderer_abi);
    memcpy(out+32,c->package_sha256,32);memcpy(out+64,c->source_sha256,32);memcpy(out+96,c->profile_sha256,32);memcpy(out+128,c->semantic_sha256,32);
    for(unsigned i=0;i<OSG_RENDER_OBJECT_KINDS;i++){uint8_t *p=out+160+i*40;p32(p,slot->offsets[i]);p32(p+4,c->objects[i].bytes);memcpy(p+8,c->objects[i].sha256,32);}
}
static osg_content_status header_decode(osg_content_store *s,osg_content_slot *slot,const uint8_t in[OSG_CONTENT_HEADER_BYTES]){
    if(memcmp(in,slot_magic,8)||u16(in+8)!=1||u16(in+10)!=OSG_CONTENT_HEADER_BYTES||u16(in+30)||!zeros(in+320,192)||!u64(in+12))return OSG_CONTENT_INVALID;
    memset(slot,0,sizeof(*slot));osg_content_candidate *c=&slot->candidate;slot->generation=u64(in+12);c->manifest_bytes=u32(in+24);c->renderer_abi=u16(in+28);
    memcpy(c->package_sha256,in+32,32);memcpy(c->source_sha256,in+64,32);memcpy(c->profile_sha256,in+96,32);memcpy(c->semantic_sha256,in+128,32);
    for(unsigned i=0;i<OSG_RENDER_OBJECT_KINDS;i++){const uint8_t *p=in+160+i*40;c->objects[i].bytes=u32(p+4);memcpy(c->objects[i].sha256,p+8,32);}
    osg_content_status status=layout(s,slot);if(status!=OSG_CONTENT_OK)return status;
    if(slot->payload_bytes!=u32(in+20))return OSG_CONTENT_INVALID;
    for(unsigned i=0;i<OSG_RENDER_OBJECT_KINDS;i++)if(slot->offsets[i]!=u32(in+160+i*40))return OSG_CONTENT_INVALID;
    return OSG_CONTENT_OK;
}
static bool hash_bytes(osg_content_store *s,const void *bytes,size_t n,uint8_t out[32]){
    return s->io.hash_begin(s->io.hash_context)&&s->io.hash_update(s->io.hash_context,bytes,n)&&s->io.hash_finish(s->io.hash_context,out);
}
static osg_content_status hash_region(osg_content_store *s,uint32_t offset,uint32_t bytes,uint8_t out[32]){
    if(!s->io.hash_begin(s->io.hash_context))return OSG_CONTENT_HASH;
    while(bytes){size_t n=bytes>OSG_CONTENT_CHUNK_BYTES?OSG_CONTENT_CHUNK_BYTES:bytes;
        if(!s->io.read(s->io.context,offset,s->scratch,n))return OSG_CONTENT_IO;
        if(!s->io.hash_update(s->io.hash_context,s->scratch,n))return OSG_CONTENT_HASH;
        offset+=(uint32_t)n;bytes-=(uint32_t)n;
    }
    return s->io.hash_finish(s->io.hash_context,out)?OSG_CONTENT_OK:OSG_CONTENT_HASH;
}
static osg_content_status verify_slot(osg_content_store *s,unsigned index){
    osg_content_slot *slot=&s->slots[index];uint8_t digest[32];
    for(unsigned part=0;part<OSG_CONTENT_PARTS;part++){
        const uint32_t bytes=part_bytes(slot,part);if(!bytes)continue;
        osg_content_status status=hash_region(s,s->slot_offsets[index]+s->io.erase_bytes+slot->offsets[part],bytes,digest);
        if(status!=OSG_CONTENT_OK)return status;
        if(!equal(digest,part==OSG_CONTENT_MANIFEST?slot->candidate.package_sha256:slot->candidate.objects[part].sha256))return OSG_CONTENT_HASH;
    }
    osg_content_view view={.store=s,.generation=slot->generation,.slot=index};s->validating_slot=(int)index;
    bool ok=s->io.validate_manifest(s->io.validation_context,&view,&slot->candidate)&&s->io.validate_content(s->io.validation_context,&view,&slot->candidate);
    s->validating_slot=-1;return ok?OSG_CONTENT_OK:OSG_CONTENT_INVALID;
}
osg_content_status osg_content_init(osg_content_store *s,osg_content_io io){
    if(!s||!io.read||!io.write||!io.erase||!io.hash_begin||!io.hash_update||!io.hash_finish||!io.validate_manifest||!io.validate_content||
        io.erase_bytes<OSG_CONTENT_HEADER_BYTES||(io.erase_bytes&(io.erase_bytes-1))||io.erase_bytes>io.bytes/6||!io.manifest_limit||io.manifest_limit>8192||!io.package_limit)return OSG_CONTENT_ARGUMENT;
    memset(s,0,sizeof(*s));s->io=io;s->active_slot=s->active_journal=s->stage_slot=s->validating_slot=-1;
    s->slot_bytes=((io.bytes-io.erase_bytes*2)/2)/io.erase_bytes*io.erase_bytes;s->slot_offsets[0]=io.erase_bytes*2;s->slot_offsets[1]=s->slot_offsets[0]+s->slot_bytes;
    return OSG_CONTENT_OK;
}
static osg_content_status journal_load(osg_content_store *s,unsigned journal,unsigned *index){
    uint8_t record[JOURNAL_BYTES],header[OSG_CONTENT_HEADER_BYTES],digest[32];
    if(!s->io.read(s->io.context,journal*s->io.erase_bytes,record,sizeof(record)))return OSG_CONTENT_IO;
    if(memcmp(record,"OSGJ",4)||u16(record+4)!=1||u16(record+6)>1||!u64(record+8)||u32(record+112)!=COMMIT_MARKER||!zeros(record+116,12))return OSG_CONTENT_INVALID;
    if(!hash_bytes(s,record,80,digest)||!equal(digest,record+80))return OSG_CONTENT_HASH;
    *index=u16(record+6);
    if(!s->io.read(s->io.context,s->slot_offsets[*index],header,sizeof(header)))return OSG_CONTENT_IO;
    if(!hash_bytes(s,header,sizeof(header),digest)||!equal(digest,record+16))return OSG_CONTENT_HASH;
    osg_content_slot loaded;osg_content_status status=header_decode(s,&loaded,header);if(status!=OSG_CONTENT_OK)return status;
    if(loaded.generation!=u64(record+8)||!equal(loaded.candidate.package_sha256,record+48))return OSG_CONTENT_INVALID;
    osg_content_slot previous=s->slots[*index];s->slots[*index]=loaded;status=verify_slot(s,*index);if(status==OSG_CONTENT_OK)s->slots[*index].valid=true;else s->slots[*index]=previous;return status;
}
osg_content_status osg_content_scan(osg_content_store *s){
    if(!s)return OSG_CONTENT_ARGUMENT;
    if(s->stage_slot>=0||s->slots[0].pins||s->slots[1].pins)return OSG_CONTENT_BUSY;
    s->active_slot=s->active_journal=-1;s->slots[0].valid=s->slots[1].valid=false;bool io_failed=false;
    for(unsigned journal=0;journal<2;journal++){
        unsigned index=0;osg_content_status status=journal_load(s,journal,&index);if(status==OSG_CONTENT_IO)io_failed=true;if(status!=OSG_CONTENT_OK)continue;
        if(s->active_slot>=0&&s->slots[index].generation==s->slots[s->active_slot].generation&&index!=(unsigned)s->active_slot){s->active_slot=s->active_journal=-1;return OSG_CONTENT_INVALID;}
        if(s->active_slot<0||s->slots[index].generation>s->slots[s->active_slot].generation){s->active_slot=(int)index;s->active_journal=(int)journal;}
    }
    return s->active_slot>=0?OSG_CONTENT_OK:io_failed?OSG_CONTENT_IO:OSG_CONTENT_NOT_FOUND;
}
osg_content_status osg_content_begin(osg_content_store *s,const osg_content_candidate *candidate){
    if(!s||!candidate)return OSG_CONTENT_ARGUMENT;
    if(s->stage_slot>=0)return OSG_CONTENT_BUSY;
    const unsigned index=s->active_slot==0?1:0;if(s->slots[index].pins)return OSG_CONTENT_BUSY;
    const uint64_t generation=s->active_slot>=0?s->slots[s->active_slot].generation:0;if(generation==UINT64_MAX)return OSG_CONTENT_EXHAUSTED;
    osg_content_slot proposed={.candidate=*candidate,.generation=generation+1};osg_content_status status=layout(s,&proposed);if(status!=OSG_CONTENT_OK)return status;
    s->slots[index]=proposed;s->stage_slot=(int)index;s->erase_progress=0;s->ready=s->verified=false;memset(s->written,0,sizeof(s->written));return OSG_CONTENT_OK;
}
osg_content_status osg_content_erase_step(osg_content_store *s,bool *done){
    if(!s||!done)return OSG_CONTENT_ARGUMENT;
    *done=false;
    if(s->stage_slot<0||s->ready)return OSG_CONTENT_STATE;
    if(!s->io.erase(s->io.context,s->slot_offsets[s->stage_slot]+s->erase_progress,s->io.erase_bytes))return OSG_CONTENT_IO;
    s->erase_progress+=s->io.erase_bytes;
    if(s->erase_progress==s->slot_bytes){uint8_t header[OSG_CONTENT_HEADER_BYTES];header_encode(&s->slots[s->stage_slot],header);
        if(!s->io.write(s->io.context,s->slot_offsets[s->stage_slot],header,sizeof(header))){osg_content_abort(s);return OSG_CONTENT_IO;}
        s->ready=true;*done=true;
    }
    return OSG_CONTENT_OK;
}
osg_content_status osg_content_write(osg_content_store *s,unsigned part,uint32_t offset,const void *bytes,size_t n){
    if(!s||part>=OSG_CONTENT_PARTS||!bytes||!n||n>OSG_CONTENT_CHUNK_BYTES)return OSG_CONTENT_ARGUMENT;
    if(s->stage_slot<0||!s->ready||s->verified)return OSG_CONTENT_STATE;
    osg_content_slot *slot=&s->slots[s->stage_slot];const uint32_t size=part_bytes(slot,part);
    if(offset!=s->written[part])return OSG_CONTENT_ORDER;
    if(offset>size||n>size-offset)return OSG_CONTENT_BUDGET;
    if(!s->io.write(s->io.context,s->slot_offsets[s->stage_slot]+s->io.erase_bytes+slot->offsets[part]+offset,bytes,n))return OSG_CONTENT_IO;
    s->written[part]+=(uint32_t)n;return OSG_CONTENT_OK;
}
osg_content_status osg_content_verify(osg_content_store *s){
    if(!s)return OSG_CONTENT_ARGUMENT;
    if(s->stage_slot<0||!s->ready)return OSG_CONTENT_STATE;
    for(unsigned i=0;i<OSG_CONTENT_PARTS;i++)if(s->written[i]!=part_bytes(&s->slots[s->stage_slot],i))return OSG_CONTENT_ORDER;
    osg_content_status status=verify_slot(s,(unsigned)s->stage_slot);s->verified=status==OSG_CONTENT_OK;return status;
}
osg_content_status osg_content_commit(osg_content_store *s){
    if(!s)return OSG_CONTENT_ARGUMENT;
    if(s->stage_slot<0||!s->ready||!s->verified)return OSG_CONTENT_STATE;
    const unsigned journal=s->active_journal==0?1:0,index=(unsigned)s->stage_slot;
    uint8_t record[JOURNAL_BYTES]={0},header[OSG_CONTENT_HEADER_BYTES],observed[JOURNAL_BYTES];
    memcpy(record,"OSGJ",4);p16(record+4,1);p16(record+6,(uint16_t)index);p64(record+8,s->slots[index].generation);
    if(!s->io.read(s->io.context,s->slot_offsets[index],header,sizeof(header)))return OSG_CONTENT_IO;
    if(!hash_bytes(s,header,sizeof(header),record+16))return OSG_CONTENT_HASH;
    memcpy(record+48,s->slots[index].candidate.package_sha256,32);
    if(!hash_bytes(s,record,80,record+80))return OSG_CONTENT_HASH;
    p32(record+112,UINT32_MAX);
    if(!s->io.erase(s->io.context,journal*s->io.erase_bytes,s->io.erase_bytes)||!s->io.write(s->io.context,journal*s->io.erase_bytes,record,sizeof(record)))return OSG_CONTENT_IO;
    p32(record+112,COMMIT_MARKER);
    if(!s->io.write(s->io.context,journal*s->io.erase_bytes+112,record+112,4)||!s->io.read(s->io.context,journal*s->io.erase_bytes,observed,sizeof(observed))||memcmp(record,observed,sizeof(record)))return OSG_CONTENT_IO;
    s->slots[index].valid=true;s->active_slot=(int)index;s->active_journal=(int)journal;osg_content_abort(s);return OSG_CONTENT_OK;
}
void osg_content_abort(osg_content_store *s){if(s){s->stage_slot=-1;s->ready=s->verified=false;s->erase_progress=0;memset(s->written,0,sizeof(s->written));}}
static bool readable(const osg_content_view *view){
    return view&&view->store&&view->slot<2&&view->store->slots[view->slot].generation==view->generation&&
        ((view->held&&view->store->slots[view->slot].pins&&view->store->slots[view->slot].valid)||view->store->validating_slot==(int)view->slot);
}
osg_content_status osg_content_acquire(osg_content_store *s,osg_content_view *view){
    if(!s||!view)return OSG_CONTENT_ARGUMENT;
    if(s->active_slot<0||!s->slots[s->active_slot].valid)return OSG_CONTENT_NOT_FOUND;
    osg_content_slot *slot=&s->slots[s->active_slot];if(slot->pins==UINT_MAX)return OSG_CONTENT_BUSY;
    slot->pins++;*view=(osg_content_view){s,slot->generation,(unsigned)s->active_slot,true};return OSG_CONTENT_OK;
}
void osg_content_release(osg_content_view *view){
    if(view&&view->held){if(readable(view))view->store->slots[view->slot].pins--;view->held=false;view->store=NULL;}
}
const osg_content_candidate *osg_content_metadata(const osg_content_view *view){return readable(view)?&view->store->slots[view->slot].candidate:NULL;}
osg_content_status osg_content_read(const osg_content_view *view,unsigned part,uint32_t offset,void *bytes,size_t n){
    if(!readable(view)||part>=OSG_CONTENT_PARTS||!bytes||!n||n>OSG_CONTENT_CHUNK_BYTES)return OSG_CONTENT_ARGUMENT;
    osg_content_store *s=view->store;const osg_content_slot *slot=&s->slots[view->slot];const uint32_t size=part_bytes(slot,part);
    if(offset>size||n>size-offset)return OSG_CONTENT_BUDGET;
    return s->io.read(s->io.context,s->slot_offsets[view->slot]+s->io.erase_bytes+slot->offsets[part]+offset,bytes,n)?OSG_CONTENT_OK:OSG_CONTENT_IO;
}
bool osg_content_object_read(void *context,uint32_t offset,void *bytes,size_t n){
    osg_content_reader *reader=context;return reader&&osg_content_read(reader->view,reader->part,offset,bytes,n)==OSG_CONTENT_OK;
}
