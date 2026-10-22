#include "../render/content_store.h"
#include <assert.h>
#include <openssl/evp.h>
#include <openssl/sha.h>
#include <stdio.h>
#include <string.h>
#define FLASH_BYTES 65536u
static uint8_t flash_bytes[FLASH_BYTES],saved[FLASH_BYTES];
static unsigned step,cut,partial,largest_read,largest_write,validations;
static bool powered=true,reject_manifest,reject_content,crypto_fail;
static EVP_MD_CTX *hash_context;
typedef struct {osg_content_candidate candidate;uint8_t manifest[128],semantics[64],scene[3001],images[2049];} fixture;
static fixture fixtures[2];
static bool event(void){step++;if(!powered)return false;if(cut&&step==cut){powered=false;return false;}return true;}
static bool flash_read(void *context,uint32_t offset,void *out,size_t n){(void)context;assert(offset<=FLASH_BYTES&&n<=FLASH_BYTES-offset&&n<=OSG_CONTENT_CHUNK_BYTES);if(n>largest_read)largest_read=(unsigned)n;if(!event())return false;memcpy(out,flash_bytes+offset,n);return true;}
static bool flash_write(void *context,uint32_t offset,const void *bytes,size_t n){(void)context;assert(offset<=FLASH_BYTES&&n<=FLASH_BYTES-offset&&n<=OSG_CONTENT_CHUNK_BYTES);if(n>largest_write)largest_write=(unsigned)n;
    bool ok=event();if(!ok&&!cut)return false;size_t apply=ok?n:powered?0:partial==2?n:partial==1?n/2:0;const uint8_t *p=bytes;
    for(size_t i=0;i<apply;i++){assert((flash_bytes[offset+i]&p[i])==p[i]);flash_bytes[offset+i]&=p[i];}return ok;}
static bool flash_erase(void *context,uint32_t offset,uint32_t n){(void)context;assert(offset%4096==0&&n==4096&&offset+n<=FLASH_BYTES);bool ok=event();size_t apply=ok?n:partial==2?n:partial==1?n/2:0;if(apply)memset(flash_bytes+offset,255,apply);return ok;}
static bool hash_begin(void *context){return event()&&!crypto_fail&&EVP_DigestInit_ex(context,EVP_sha256(),NULL)==1;}
static bool hash_update(void *context,const void *bytes,size_t n){assert(n<=OSG_CONTENT_CHUNK_BYTES);return event()&&!crypto_fail&&EVP_DigestUpdate(context,bytes,n)==1;}
static bool hash_finish(void *context,uint8_t out[32]){unsigned n=0;return event()&&!crypto_fail&&EVP_DigestFinal_ex(context,out,&n)==1&&n==32;}
static const fixture *find_fixture(const osg_content_candidate *c){for(unsigned i=0;i<2;i++)if(!memcmp(c->package_sha256,fixtures[i].candidate.package_sha256,32))return &fixtures[i];return NULL;}
static const uint8_t *part_data(const fixture *f,unsigned part){return part==OSG_CONTENT_MANIFEST?f->manifest:part==0?f->semantics:part==1?f->scene:f->images;}
static uint32_t part_size(const fixture *f,unsigned part){return part==OSG_CONTENT_MANIFEST?f->candidate.manifest_bytes:f->candidate.objects[part].bytes;}
static bool validate_manifest(void *context,const osg_content_view *view,const osg_content_candidate *candidate){
    (void)context;validations++;if(!event()||reject_manifest)return false;const fixture *f=find_fixture(candidate);if(!f||memcmp(candidate,&f->candidate,sizeof(*candidate)))return false;
    uint8_t manifest[128];return osg_content_read(view,OSG_CONTENT_MANIFEST,0,manifest,sizeof(manifest))==OSG_CONTENT_OK&&!memcmp(manifest,f->manifest,sizeof(manifest));}
static bool validate_content(void *context,const osg_content_view *view,const osg_content_candidate *candidate){
    (void)context;if(!event()||reject_content)return false;const fixture *f=find_fixture(candidate);if(!f)return false;
    uint8_t data[2048];for(unsigned part=0;part<OSG_RENDER_OBJECT_KINDS;part++)for(uint32_t offset=0;offset<part_size(f,part);){size_t n=part_size(f,part)-offset;if(n>sizeof(data))n=sizeof(data);
        if(osg_content_read(view,part,offset,data,n)!=OSG_CONTENT_OK||memcmp(data,part_data(f,part)+offset,n))return false;
        offset+=(uint32_t)n;}return true;}
static osg_content_io io(void){return (osg_content_io){.bytes=FLASH_BYTES,.erase_bytes=4096,.manifest_limit=4096,.package_limit=20000,.read=flash_read,.write=flash_write,.erase=flash_erase,
    .hash_context=hash_context,.hash_begin=hash_begin,.hash_update=hash_update,.hash_finish=hash_finish,.validate_manifest=validate_manifest,.validate_content=validate_content};}
static void fixture_init(unsigned index){fixture *f=&fixtures[index];memset(f,0,sizeof(*f));memset(f->manifest,'A'+index,sizeof(f->manifest));memset(f->semantics,30+index,sizeof(f->semantics));
    for(unsigned i=0;i<sizeof(f->scene);i++)f->scene[i]=(uint8_t)(i+index);
    memset(f->images,150+index,sizeof(f->images));osg_content_candidate *c=&f->candidate;
    c->manifest_bytes=sizeof(f->manifest);c->renderer_abi=1;SHA256(f->manifest,sizeof(f->manifest),c->package_sha256);SHA256((const unsigned char *)"source",6,c->source_sha256);SHA256((const unsigned char *)"profile",7,c->profile_sha256);
    c->objects[0].bytes=sizeof(f->semantics);c->objects[1].bytes=sizeof(f->scene);c->objects[2].bytes=sizeof(f->images);
    for(unsigned part=0;part<3;part++)SHA256(part_data(f,part),part_size(f,part),c->objects[part].sha256);
    memcpy(c->semantic_sha256,c->objects[0].sha256,32);}
static osg_content_status install(osg_content_store *store,const fixture *f){
    osg_content_status status=osg_content_begin(store,&f->candidate);if(status!=OSG_CONTENT_OK)return status;bool done=false;
    while(!done){status=osg_content_erase_step(store,&done);if(status!=OSG_CONTENT_OK)return status;}
    for(unsigned part=0;part<OSG_CONTENT_PARTS;part++)for(uint32_t offset=0;offset<part_size(f,part);){size_t n=part_size(f,part)-offset;if(n>2048)n=2048;status=osg_content_write(store,part,offset,part_data(f,part)+offset,n);if(status!=OSG_CONTENT_OK)return status;offset+=(uint32_t)n;}
    status=osg_content_verify(store);return status==OSG_CONTENT_OK?osg_content_commit(store):status;}
static void reset_faults(void){powered=true;cut=0;step=0;crypto_fail=reject_content=reject_manifest=false;}
static unsigned current_fixture(osg_content_store *store){osg_content_view view;assert(osg_content_acquire(store,&view)==OSG_CONTENT_OK);const osg_content_candidate *c=osg_content_metadata(&view);const fixture *f=find_fixture(c);assert(f&&validate_content(NULL,&view,c));unsigned index=(unsigned)(f-fixtures);osg_content_release(&view);return index;}
static void write_all(osg_content_store *s,const fixture *f){for(unsigned part=0;part<OSG_CONTENT_PARTS;part++)for(uint32_t offset=0;offset<part_size(f,part);){size_t n=part_size(f,part)-offset;if(n>2048)n=2048;assert(osg_content_write(s,part,offset,part_data(f,part)+offset,n)==OSG_CONTENT_OK);offset+=(uint32_t)n;}}
int main(void){
    hash_context=EVP_MD_CTX_new();assert(hash_context);fixture_init(0);fixture_init(1);memset(flash_bytes,255,sizeof(flash_bytes));osg_content_store store,reboot;
    assert(osg_content_init(NULL,io())==OSG_CONTENT_ARGUMENT);osg_content_io bad=io();bad.read=NULL;assert(osg_content_init(&store,bad)==OSG_CONTENT_ARGUMENT);
    bad=io();bad.erase_bytes=3000;assert(osg_content_init(&store,bad)==OSG_CONTENT_ARGUMENT);bad=io();bad.bytes=100;assert(osg_content_init(&store,bad)==OSG_CONTENT_ARGUMENT);
    assert(osg_content_init(&store,io())==OSG_CONTENT_OK&&sizeof(store)<=4096);assert(osg_content_scan(&store)==OSG_CONTENT_NOT_FOUND);
    assert(osg_content_scan(NULL)==OSG_CONTENT_ARGUMENT);osg_content_view old={0};assert(osg_content_acquire(&store,&old)==OSG_CONTENT_NOT_FOUND);assert(osg_content_acquire(NULL,&old)==OSG_CONTENT_ARGUMENT);
    assert(!osg_content_metadata(NULL)&&!osg_content_metadata(&old)&&!osg_content_object_read(NULL,0,saved,1));osg_content_release(NULL);osg_content_release(&old);
    assert(osg_content_read(&old,0,0,saved,1)==OSG_CONTENT_ARGUMENT);bool done=false;
    assert(osg_content_begin(NULL,&fixtures[0].candidate)==OSG_CONTENT_ARGUMENT);assert(osg_content_erase_step(&store,NULL)==OSG_CONTENT_ARGUMENT);
    assert(osg_content_erase_step(&store,&done)==OSG_CONTENT_STATE&&osg_content_verify(&store)==OSG_CONTENT_STATE&&osg_content_commit(&store)==OSG_CONTENT_STATE);
    assert(osg_content_verify(NULL)==OSG_CONTENT_ARGUMENT&&osg_content_commit(NULL)==OSG_CONTENT_ARGUMENT);osg_content_abort(NULL);
    osg_content_candidate invalid=fixtures[0].candidate;invalid.renderer_abi=2;assert(osg_content_begin(&store,&invalid)==OSG_CONTENT_INVALID);
    invalid=fixtures[0].candidate;invalid.manifest_bytes=0;assert(osg_content_begin(&store,&invalid)==OSG_CONTENT_INVALID);
    invalid=fixtures[0].candidate;invalid.objects[0].bytes=0;assert(osg_content_begin(&store,&invalid)==OSG_CONTENT_INVALID);
    invalid=fixtures[0].candidate;invalid.semantic_sha256[0]^=1;assert(osg_content_begin(&store,&invalid)==OSG_CONTENT_INVALID);
    invalid=fixtures[0].candidate;invalid.objects[3].sha256[0]=1;assert(osg_content_begin(&store,&invalid)==OSG_CONTENT_INVALID);
    invalid=fixtures[0].candidate;invalid.objects[2].bytes=UINT32_MAX;assert(osg_content_begin(&store,&invalid)==OSG_CONTENT_BUDGET);
    assert(install(&store,&fixtures[0])==OSG_CONTENT_OK&&current_fixture(&store)==0);memcpy(saved,flash_bytes,sizeof(saved));
    assert(osg_content_acquire(&store,&old)==OSG_CONTENT_OK);osg_content_reader reader={&old,0};uint8_t first=0;
    assert(osg_content_object_read(&reader,0,&first,1)&&first==fixtures[0].semantics[0]);assert(osg_content_read(&old,0,100,&first,1)==OSG_CONTENT_BUDGET);
    assert(osg_content_begin(&store,&fixtures[1].candidate)==OSG_CONTENT_OK&&osg_content_begin(&store,&fixtures[1].candidate)==OSG_CONTENT_BUSY);
    assert(osg_content_scan(&store)==OSG_CONTENT_BUSY&&osg_content_write(&store,0,0,&first,1)==OSG_CONTENT_STATE);
    while(!done)assert(osg_content_erase_step(&store,&done)==OSG_CONTENT_OK);
    assert(osg_content_erase_step(&store,&done)==OSG_CONTENT_STATE&&osg_content_verify(&store)==OSG_CONTENT_ORDER);
    assert(osg_content_write(&store,0,1,&first,1)==OSG_CONTENT_ORDER&&osg_content_write(&store,5,0,&first,1)==OSG_CONTENT_ARGUMENT);
    assert(osg_content_write(&store,0,0,&first,2049)==OSG_CONTENT_ARGUMENT&&osg_content_write(&store,0,0,NULL,1)==OSG_CONTENT_ARGUMENT);
    assert(osg_content_write(&store,0,0,fixtures[0].scene,65)==OSG_CONTENT_BUDGET&&osg_content_write(&store,3,0,&first,1)==OSG_CONTENT_BUDGET);
    write_all(&store,&fixtures[1]);uint32_t damaged=store.slot_offsets[store.stage_slot]+4096+store.slots[store.stage_slot].offsets[1];flash_bytes[damaged]^=1;
    assert(osg_content_verify(&store)==OSG_CONTENT_HASH);flash_bytes[damaged]^=1;reject_manifest=true;assert(osg_content_verify(&store)==OSG_CONTENT_INVALID);reject_manifest=false;
    reject_content=true;assert(osg_content_verify(&store)==OSG_CONTENT_INVALID);reject_content=false;crypto_fail=true;assert(osg_content_verify(&store)==OSG_CONTENT_HASH);crypto_fail=false;
    assert(osg_content_verify(&store)==OSG_CONTENT_OK&&osg_content_write(&store,0,0,&first,1)==OSG_CONTENT_STATE);
    assert(osg_content_commit(&store)==OSG_CONTENT_OK&&current_fixture(&store)==1&&osg_content_object_read(&reader,0,&first,1)&&first==fixtures[0].semantics[0]);
    assert(osg_content_begin(&store,&fixtures[0].candidate)==OSG_CONTENT_BUSY&&osg_content_scan(&store)==OSG_CONTENT_BUSY);
    osg_content_release(&old);osg_content_release(&old);assert(!osg_content_object_read(&reader,0,&first,1));assert(osg_content_begin(&store,&fixtures[0].candidate)==OSG_CONTENT_OK);osg_content_abort(&store);
    flash_bytes[damaged]^=1;assert(osg_content_init(&reboot,io())==OSG_CONTENT_OK&&osg_content_scan(&reboot)==OSG_CONTENT_OK&&current_fixture(&reboot)==0);flash_bytes[damaged]^=1;
    /* Exercise every event boundary including erase, chunk writes, object/full
     * hashes, semantic validation and journal/marker commit. Torn writes model
     * no bytes, a prefix, or all bytes persisted before the power loss. */
    memcpy(flash_bytes,saved,sizeof(saved));reset_faults();assert(osg_content_init(&store,io())==OSG_CONTENT_OK&&osg_content_scan(&store)==OSG_CONTENT_OK);step=0;assert(install(&store,&fixtures[1])==OSG_CONTENT_OK);unsigned events=step,runs=0;
    for(partial=0;partial<3;partial++)for(unsigned point=1;point<=events;point++){
        memcpy(flash_bytes,saved,sizeof(saved));reset_faults();assert(osg_content_init(&store,io())==OSG_CONTENT_OK&&osg_content_scan(&store)==OSG_CONTENT_OK);step=0;cut=point;
        assert(install(&store,&fixtures[1])!=OSG_CONTENT_OK);reset_faults();assert(osg_content_init(&reboot,io())==OSG_CONTENT_OK&&osg_content_scan(&reboot)==OSG_CONTENT_OK);assert(current_fixture(&reboot)<=1);runs++;
    }
    memset(flash_bytes,255,sizeof(flash_bytes));reset_faults();assert(osg_content_init(&store,io())==OSG_CONTENT_OK);step=0;assert(install(&store,&fixtures[0])==OSG_CONTENT_OK);events=step;
    for(partial=0;partial<3;partial++)for(unsigned point=1;point<=events;point++){
        memset(flash_bytes,255,sizeof(flash_bytes));reset_faults();assert(osg_content_init(&store,io())==OSG_CONTENT_OK);cut=point;
        assert(install(&store,&fixtures[0])!=OSG_CONTENT_OK);reset_faults();assert(osg_content_init(&reboot,io())==OSG_CONTENT_OK);osg_content_status status=osg_content_scan(&reboot);assert(status==OSG_CONTENT_NOT_FOUND||status==OSG_CONTENT_OK);if(status==OSG_CONTENT_OK)assert(current_fixture(&reboot)==0);runs++;
    }
    assert(largest_read<=2048&&largest_write<=2048&&validations>0);EVP_MD_CTX_free(hash_context);
    printf("Content-store streaming SHA256, pinned readers and %u power-cut recovery cases passed; max read/write %u/%u bytes\n",runs,largest_read,largest_write);return 0;
}
