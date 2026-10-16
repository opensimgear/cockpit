#include "../render/package.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint8_t bytes[64 * 1024];
static uint32_t bytes_count;
static unsigned reads, fail_read;
static const uint16_t strides[4][16] = {
    {0,32,32,24,1}, {0,16,64,24,16,16,24,8,24,4,1,12,2,8},
    {0,24,1}, {0,16,24,1}
};
static const char *const magics[] = {"OSGC", "OSGS", "OSGI", "OSGF"};

static void put16(uint32_t at, uint16_t value) {
    bytes[at]=(uint8_t)value;bytes[at+1]=(uint8_t)(value>>8);
}
static void put32(uint32_t at, uint32_t value) {
    for(unsigned i=0;i<4;i++)bytes[at+i]=(uint8_t)(value>>(8*i));
}
static bool read_bytes(void *context, uint32_t offset, void *destination, size_t count) {
    assert(context==bytes&&count<=32&&offset<=bytes_count&&count<=bytes_count-offset);
    if(++reads==fail_read)return false;
    memcpy(destination,bytes+offset,count);return true;
}
static void build(osg_render_object_kind kind, const uint16_t *sections, const uint32_t *counts, unsigned n) {
    memset(bytes,0,sizeof(bytes));memcpy(bytes,magics[kind],4);
    put16(4,1);put16(6,32);put32(12,32);put16(16,(uint16_t)n);
    uint32_t cursor=32+n*16;
    for(unsigned i=0;i<n;i++){
        cursor=(cursor+3)&~3u;unsigned at=32+i*16;
        uint16_t stride=strides[kind][sections[i]];
        assert(counts[i]&&stride&&cursor+counts[i]*stride<=sizeof(bytes));
        put16(at,sections[i]);put16(at+2,stride);put32(at+4,cursor);
        put32(at+8,counts[i]);put32(at+12,counts[i]*stride);cursor+=counts[i]*stride;
    }
    bytes_count=cursor;put32(8,bytes_count);reads=fail_read=0;
}
static void one(osg_render_object_kind kind, uint16_t section, uint32_t count) {
    build(kind,&section,&count,1);
}
static osg_render_package_status inspect(osg_render_object_kind kind, const osg_render_package_limits *limits) {
    osg_render_object_info output;memset(&output,0xa5,sizeof(output));reads=0;
    osg_render_package_status status=osg_render_object_inspect(read_bytes,bytes,bytes_count,kind,limits,&output);
    if(status==OSG_RENDER_PACKAGE_OK){
        assert(output.kind==kind&&output.object_bytes==bytes_count&&output.section_count);
    } else {
        const uint8_t *raw=(const uint8_t *)&output;
        for(size_t i=0;i<sizeof(output);i++)assert(!raw[i]);
    }
    return status;
}
static void valid_tables(void) {
    for(unsigned kind=0;kind<4;kind++){
        uint16_t sections[16];uint32_t counts[16];unsigned n=0;
        for(unsigned id=1;id<16;id++){
            assert(osg_render_section_record_bytes((osg_render_object_kind)kind,(uint16_t)id)==strides[kind][id]);
            if(strides[kind][id]){sections[n]=(uint16_t)id;counts[n++]=1;}
        }
        build((osg_render_object_kind)kind,sections,counts,n);
        assert(inspect((osg_render_object_kind)kind,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_OK);
        assert(inspect((osg_render_object_kind)kind,&OSG_RENDER_WAVESHARE_LIMITS)==OSG_RENDER_PACKAGE_OK);
    }
    assert(!osg_render_section_record_bytes((osg_render_object_kind)-1,1));
    assert(!osg_render_section_record_bytes(OSG_RENDER_OBJECT_KINDS,1));
    assert(!osg_render_section_record_bytes(OSG_RENDER_SCENE,16));
}
static void invalid_header(void) {
    one(OSG_RENDER_SCENE,1,1);osg_render_object_info output;
    assert(osg_render_object_inspect(NULL,bytes,bytes_count,OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS,&output)==OSG_RENDER_PACKAGE_ARGUMENT);
    assert(osg_render_object_inspect(read_bytes,bytes,bytes_count,OSG_RENDER_SCENE,NULL,&output)==OSG_RENDER_PACKAGE_ARGUMENT);
    assert(osg_render_object_inspect(read_bytes,bytes,bytes_count,OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS,NULL)==OSG_RENDER_PACKAGE_ARGUMENT);
    assert(inspect((osg_render_object_kind)-1,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_ARGUMENT);
    assert(inspect(OSG_RENDER_OBJECT_KINDS,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_ARGUMENT);
    uint32_t original=bytes_count;bytes_count=31;assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SIZE);
    bytes_count=original;put32(8,original+1);assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SIZE);
    const unsigned bad_header[]={0,4,6,18,20,31};
    for(unsigned i=0;i<sizeof(bad_header)/sizeof(*bad_header);i++){
        one(OSG_RENDER_SCENE,1,1);bytes[bad_header[i]]^=1;
        assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_HEADER);
    }
    one(OSG_RENDER_SCENE,1,1);put32(12,UINT32_MAX);
    assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_DIRECTORY);
    for(unsigned count=0;count<=17;count+=17){
        one(OSG_RENDER_SCENE,1,1);put16(16,(uint16_t)count);
        assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_DIRECTORY);
    }
    one(OSG_RENDER_SCENE,1,1);put16(16,13);
    assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_DIRECTORY);
}
static void invalid_sections(void) {
    const uint16_t bad_ids[]={0,14,16,65535};
    for(unsigned i=0;i<sizeof(bad_ids)/sizeof(*bad_ids);i++){
        one(OSG_RENDER_SCENE,1,1);put16(32,bad_ids[i]);
        assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SECTION);
    }
    const uint16_t ids[]={1,2};const uint32_t counts[]={1,1};
    build(OSG_RENDER_SCENE,ids,counts,2);put16(48,1);
    assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SECTION);
    one(OSG_RENDER_SCENE,1,1);put16(34,0);assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SECTION);
    one(OSG_RENDER_SCENE,1,1);put16(34,32);assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SECTION);
    one(OSG_RENDER_SCENE,1,1);put32(40,0);assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SECTION);
    one(OSG_RENDER_SCENE,1,1);put32(40,UINT32_MAX);assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SECTION);
    one(OSG_RENDER_SCENE,1,1);put32(44,17);assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SECTION);
    const uint32_t offsets[]={0,32,49,52,UINT32_MAX};
    for(unsigned i=0;i<sizeof(offsets)/sizeof(*offsets);i++){
        one(OSG_RENDER_SCENE,1,1);put32(36,offsets[i]);
        assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_OVERLAP);
    }
    one(OSG_RENDER_SCENE,1,1);bytes_count--;put32(8,bytes_count);
    assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SIZE);
    one(OSG_RENDER_SCENE,1,1);bytes_count++;put32(8,bytes_count);
    assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SIZE);
    const uint16_t padded[]={10,11};
    build(OSG_RENDER_SCENE,padded,counts,2);bytes[65]=1;
    assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SECTION);
    build(OSG_RENDER_SCENE,padded,counts,2);assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_OK);
    unsigned calls=reads;
    for(unsigned n=1;n<=calls;n++){
        fail_read=n;assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_READ);
    }
    fail_read=0;
    /* Exhausted object ending after an odd-byte section cannot provide the
     * mandatory alignment before its next declared section. */
    build(OSG_RENDER_SCENE,padded,counts,2);bytes_count=65;put32(8,bytes_count);
    assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_SIZE);
}
static void budgets(void) {
    const uint16_t scene_ids[]={1,2,3,4,5,6,8};
    const uint32_t caps[]={4,128,32,128,512,4,32};
    for(unsigned i=0;i<sizeof(scene_ids)/sizeof(*scene_ids);i++){
        one(OSG_RENDER_SCENE,scene_ids[i],caps[i]);assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_OK);
        one(OSG_RENDER_SCENE,scene_ids[i],caps[i]+1);assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_BUDGET);
    }
    for(uint16_t id=1;id<=3;id++){
        one(OSG_RENDER_SEMANTICS,id,33);assert(inspect(OSG_RENDER_SEMANTICS,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_BUDGET);
    }
    one(OSG_RENDER_FONTS,2,257);assert(inspect(OSG_RENDER_FONTS,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_BUDGET);
    assert(inspect(OSG_RENDER_FONTS,&OSG_RENDER_WAVESHARE_LIMITS)==OSG_RENDER_PACKAGE_OK);
    one(OSG_RENDER_SCENE,2,129);assert(inspect(OSG_RENDER_SCENE,&OSG_RENDER_WAVESHARE_LIMITS)==OSG_RENDER_PACKAGE_OK);
    osg_render_package_limits limits=OSG_RENDER_SUNTON_LIMITS;
    one(OSG_RENDER_SCENE,1,1);limits.object_bytes[OSG_RENDER_SCENE]=bytes_count-1;
    assert(inspect(OSG_RENDER_SCENE,&limits)==OSG_RENDER_PACKAGE_BUDGET);
}
static void golden(const char *path, osg_render_object_kind kind) {
    FILE *file=fopen(path,"rb");assert(file);
    bytes_count=(uint32_t)fread(bytes,1,sizeof(bytes),file);assert(bytes_count&&!ferror(file)&&feof(file));fclose(file);
    fail_read=0;assert(inspect(kind,&OSG_RENDER_SUNTON_LIMITS)==OSG_RENDER_PACKAGE_OK);
    assert(inspect(kind,&OSG_RENDER_WAVESHARE_LIMITS)==OSG_RENDER_PACKAGE_OK);
}
int main(int argc, char **argv) {
    if(argc==2&&!strcmp(argv[1],"--abi")){
        const char *const names[]={"semantics","scene","images","fonts"};
        printf("{\"headerBytes\":%u,\"directoryBytes\":%u,\"recordBytes\":{",OSG_RENDER_OBJECT_HEADER_BYTES,OSG_RENDER_DIRECTORY_RECORD_BYTES);
        for(unsigned kind=0;kind<4;kind++){
            printf("%s\"%s\":{",kind?",":"",names[kind]);bool first=true;
            for(uint16_t id=1;id<16;id++)if(strides[kind][id]){
                printf("%s\"%u\":%u",first?"":",",id,osg_render_section_record_bytes((osg_render_object_kind)kind,id));first=false;
            }
            printf("}");
        }
        puts("}}");return 0;
    }
    assert(argc==1||argc==5);valid_tables();invalid_header();invalid_sections();budgets();
    if(argc==5){
        for(unsigned i=0;i<4;i++)golden(argv[i+1],(osg_render_object_kind)i);
        puts("Independent TypeScript golden objects accepted on both device profiles");
    }
    puts("Render object streaming bounds, canonical layout, profile counts and malformed inputs passed");
    return 0;
}
