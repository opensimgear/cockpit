#include "../render/expressions.h"
#include <assert.h>
#include <float.h>
#include <limits.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

#define LOOKUP_OFFSET 16384u
static uint8_t bytes[32768];
static osg_expression_variable samples[64];
static uint16_t sample_count;
static uint32_t lookup_count;
static unsigned reads, fail_read;
static void put16(unsigned offset, uint16_t value) {
    bytes[offset]=(uint8_t)value;bytes[offset+1]=(uint8_t)(value>>8);
}
static void put32(unsigned offset, int32_t value) {
    uint32_t raw=(uint32_t)value;
    for(unsigned i=0;i<4;i++)bytes[offset+i]=(uint8_t)(raw>>(i*8));
}
static void reset(void) {
    memset(bytes,0,sizeof(bytes));memset(samples,0,sizeof(samples));
    reads=fail_read=sample_count=lookup_count=0;
}
static void instruction(unsigned index, unsigned op, uint16_t operand, int32_t value) {
    assert(index<OSG_EXPRESSION_MAX_INSTRUCTIONS);unsigned offset=index*16;
    bytes[offset]=(uint8_t)op;put16(offset+2,operand);put32(offset+4,value);
}
static void point(unsigned index,int32_t x,int32_t y) {
    assert(LOOKUP_OFFSET+(index+1)*8<=sizeof(bytes));
    put32(LOOKUP_OFFSET+index*8,x);put32(LOOKUP_OFFSET+index*8+4,y);
    if(index>=lookup_count)lookup_count=index+1;
}
static void sample(unsigned index,osg_expression_type type,bool available,double value) {
    assert(index<sizeof(samples)/sizeof(*samples));
    samples[index]=(osg_expression_variable){.type=type,.available=available,.value=value};
    if(index>=sample_count)sample_count=(uint16_t)(index+1);
}
static bool read_bytes(void *context,uint32_t offset,void *destination,size_t count) {
    assert(context==bytes&&(count==8||count==16)&&offset<=sizeof(bytes)&&count<=sizeof(bytes)-offset);
    if(++reads==fail_read)return false;
    memcpy(destination,bytes+offset,count);return true;
}
static osg_expression_program program(uint16_t count) {
    return (osg_expression_program){.read=read_bytes,.context=bytes,.object_bytes=sizeof(bytes),
        .instruction_count=count,.lookup_offset=LOOKUP_OFFSET,.lookup_count=lookup_count};
}
static osg_expression_status run(uint16_t count,int32_t *output) {
    osg_expression_program p=program(count);reads=0;
    return osg_expression_evaluate(&p,samples,sample_count,output);
}
static void expect(uint16_t count,osg_expression_status status,int32_t value) {
    int32_t output=123456789;osg_expression_status actual=run(count,&output);
    assert(actual==status&&output==(status==OSG_EXPRESSION_OK?value:0));
}
static void golden_expect(const char *name,uint16_t count,bool valid,int32_t expected) {
    int32_t output=123456789;osg_expression_status actual=run(count,&output);
    if((actual==OSG_EXPRESSION_OK)!=valid||(valid&&output!=expected)||(!valid&&output)){
        fprintf(stderr,"Golden expression '%s' failed: status=%u value=%d expected-valid=%u expected=%d\n",name,(unsigned)actual,output,valid,expected);
        assert(false);
    }
}
static void conversions(void) {
    int32_t output=0;
    assert(osg_expression_from_number(0,NULL)==OSG_EXPRESSION_ARGUMENT);
    const double inputs[]={0,-0.0,1.5,-1.5,(double)INT32_MIN/65536,(double)INT32_MAX/65536,32767.9999999,-32768.000001};
    const int32_t expected[]={0,0,98304,-98304,INT32_MIN,INT32_MAX,INT32_MAX,INT32_MIN};
    for(unsigned i=0;i<sizeof(inputs)/sizeof(*inputs);i++)assert(osg_expression_from_number(inputs[i],&output)==OSG_EXPRESSION_OK&&output==expected[i]);
    const double bad[]={NAN,INFINITY,-INFINITY};
    for(unsigned i=0;i<sizeof(bad)/sizeof(*bad);i++)assert(osg_expression_from_number(bad[i],&output)==OSG_EXPRESSION_INVALID_VALUE&&!output);
    const double overflow[]={DBL_MAX,32768,-32769};
    for(unsigned i=0;i<sizeof(overflow)/sizeof(*overflow);i++)assert(osg_expression_from_number(overflow[i],&output)==OSG_EXPRESSION_OVERFLOW&&!output);
}
static void malformed(void) {
    reset();instruction(0,1,0,INT32_MIN);expect(1,OSG_EXPRESSION_OK,INT32_MIN);
    instruction(0,1,0,INT32_MAX);expect(1,OSG_EXPRESSION_OK,INT32_MAX);
    const unsigned reserved[]={1,8,9,10,11,12,13,14,15};
    for(unsigned i=0;i<sizeof(reserved)/sizeof(*reserved);i++){
        reset();instruction(0,1,0,1);bytes[reserved[i]]=1;expect(1,OSG_EXPRESSION_MALFORMED,0);
    }
    for(unsigned op=0;op<=255;op+=17){reset();instruction(0,op,0,0);expect(1,OSG_EXPRESSION_MALFORMED,0);}
    reset();instruction(0,1,1,0);expect(1,OSG_EXPRESSION_MALFORMED,0);
    reset();instruction(0,2,0,1);expect(1,OSG_EXPRESSION_MALFORMED,0);
    reset();instruction(0,3,1,0);expect(1,OSG_EXPRESSION_MALFORMED,0);
    reset();instruction(0,3,0,1);expect(1,OSG_EXPRESSION_MALFORMED,0);
    reset();instruction(0,3,0,0);expect(1,OSG_EXPRESSION_MALFORMED,0);
    reset();instruction(0,1,0,0);instruction(1,1,0,0);expect(2,OSG_EXPRESSION_MALFORMED,0);
    reset();for(unsigned i=0;i<9;i++)instruction(i,1,0,(int32_t)i);expect(9,OSG_EXPRESSION_MALFORMED,0);
    reset();for(unsigned i=0;i<8;i++)instruction(i,1,0,(int32_t)i);for(unsigned i=8;i<15;i++)instruction(i,3,0,0);expect(15,OSG_EXPRESSION_OK,28);
    reset();instruction(0,1,0,0);for(unsigned i=1;i<1024;i++)instruction(i,13,0,0);expect(1024,OSG_EXPRESSION_OK,65536);
    expect(0,OSG_EXPRESSION_ARGUMENT,0);expect(1025,OSG_EXPRESSION_ARGUMENT,0);
    osg_expression_program p=program(1);int32_t output=99;
    assert(osg_expression_evaluate(NULL,NULL,0,&output)==OSG_EXPRESSION_ARGUMENT&&!output);
    assert(osg_expression_evaluate(&p,NULL,0,NULL)==OSG_EXPRESSION_ARGUMENT);
    p.read=NULL;assert(osg_expression_evaluate(&p,NULL,0,&output)==OSG_EXPRESSION_ARGUMENT);
    p=program(1);assert(osg_expression_evaluate(&p,NULL,1,&output)==OSG_EXPRESSION_ARGUMENT);
    p.instruction_offset=1;assert(osg_expression_evaluate(&p,NULL,0,&output)==OSG_EXPRESSION_ARGUMENT);
    p.instruction_offset=UINT32_MAX;assert(osg_expression_evaluate(&p,NULL,0,&output)==OSG_EXPRESSION_ARGUMENT);
    p=program(1);p.object_bytes=15;assert(osg_expression_evaluate(&p,NULL,0,&output)==OSG_EXPRESSION_ARGUMENT);
    p=program(1);p.lookup_count=65536;assert(osg_expression_evaluate(&p,NULL,0,&output)==OSG_EXPRESSION_ARGUMENT);
    p.lookup_count=1;p.lookup_offset=LOOKUP_OFFSET+1;assert(osg_expression_evaluate(&p,NULL,0,&output)==OSG_EXPRESSION_ARGUMENT);
    p.lookup_offset=UINT32_MAX;assert(osg_expression_evaluate(&p,NULL,0,&output)==OSG_EXPRESSION_ARGUMENT);
    p=program(1);p.lookup_count=1;p.lookup_offset=sizeof(bytes)-7;assert(osg_expression_evaluate(&p,NULL,0,&output)==OSG_EXPRESSION_ARGUMENT);
    reset();instruction(0,1,0,1);fail_read=1;expect(1,OSG_EXPRESSION_READ,0);
}
static void variables(void) {
    reset();instruction(0,2,0,0);expect(1,OSG_EXPRESSION_MALFORMED,0);
    sample(0,OSG_EXPRESSION_NUMBER,false,1);expect(1,OSG_EXPRESSION_UNAVAILABLE,0);
    sample(0,OSG_EXPRESSION_NUMBER,true,1.25);expect(1,OSG_EXPRESSION_OK,81920);
    sample(0,OSG_EXPRESSION_NUMBER,true,NAN);expect(1,OSG_EXPRESSION_INVALID_VALUE,0);
    sample(0,OSG_EXPRESSION_NUMBER,true,INFINITY);expect(1,OSG_EXPRESSION_INVALID_VALUE,0);
    sample(0,OSG_EXPRESSION_NUMBER,true,32768);expect(1,OSG_EXPRESSION_OVERFLOW,0);
    sample(0,OSG_EXPRESSION_TEXT,true,0);expect(1,OSG_EXPRESSION_INVALID_VALUE,0);
    sample(0,(osg_expression_type)99,true,0);expect(1,OSG_EXPRESSION_INVALID_VALUE,0);
    sample(0,OSG_EXPRESSION_BOOLEAN,true,0);expect(1,OSG_EXPRESSION_OK,0);
    sample(0,OSG_EXPRESSION_BOOLEAN,true,1);expect(1,OSG_EXPRESSION_OK,65536);
    sample(0,OSG_EXPRESSION_BOOLEAN,true,0.5);expect(1,OSG_EXPRESSION_INVALID_BOOLEAN,0);
    sample(0,OSG_EXPRESSION_BOOLEAN,true,NAN);expect(1,OSG_EXPRESSION_INVALID_BOOLEAN,0);
}
static void binary(unsigned op,int32_t left,int32_t right,osg_expression_status status,int32_t result) {
    reset();instruction(0,1,0,left);instruction(1,1,0,right);instruction(2,op,0,0);expect(3,status,result);
}
static void arithmetic(void) {
    binary(3,INT32_MAX,1,OSG_EXPRESSION_OVERFLOW,0);binary(4,INT32_MIN,1,OSG_EXPRESSION_OVERFLOW,0);
    binary(5,INT32_MAX,131072,OSG_EXPRESSION_OVERFLOW,0);binary(6,INT32_MIN,-65536,OSG_EXPRESSION_OVERFLOW,0);
    binary(6,65536,0,OSG_EXPRESSION_DIVISION_BY_ZERO,0);binary(6,-1,196608,OSG_EXPRESSION_OK,0);
    binary(5,-1,32768,OSG_EXPRESSION_OK,0);
    for(unsigned op=7;op<=12;op++)for(unsigned left=0;left<2;left++)for(unsigned right=0;right<2;right++){
        int32_t a=(int32_t)left*65536,b=(int32_t)right*65536,result=op==7?(a<b?a:b):op==8?(a>b?a:b):
            op==9?(a<b?65536:0):op==10?(a==b?65536:0):op==11?(a&&b?65536:0):(a||b?65536:0);
        binary(op,a,b,OSG_EXPRESSION_OK,result);
    }
    binary(11,1,65536,OSG_EXPRESSION_INVALID_BOOLEAN,0);binary(12,65536,1,OSG_EXPRESSION_INVALID_BOOLEAN,0);
    reset();instruction(0,1,0,1);instruction(1,13,0,0);expect(2,OSG_EXPRESSION_INVALID_BOOLEAN,0);
    reset();instruction(0,1,0,1);instruction(1,1,0,10);instruction(2,1,0,20);instruction(3,14,0,0);expect(4,OSG_EXPRESSION_INVALID_BOOLEAN,0);
    for(int32_t input=-1;input<=3;input++){
        reset();instruction(0,1,0,input);instruction(1,1,0,0);instruction(2,1,0,2);instruction(3,15,0,0);expect(4,OSG_EXPRESSION_OK,input<0?0:input>2?2:input);
    }
    reset();instruction(0,1,0,0);instruction(1,1,0,2);instruction(2,1,0,1);instruction(3,15,0,0);expect(4,OSG_EXPRESSION_MALFORMED,0);
}
static void lookups(void) {
    reset();point(0,INT32_MIN,INT32_MAX);point(1,INT32_MAX,INT32_MIN);
    instruction(0,1,0,INT32_MAX-1);instruction(1,16,0,2);expect(2,OSG_EXPRESSION_OK,INT32_MIN+1);
    for(int32_t input=-1;input<=3;input++){
        reset();point(0,0,10);point(1,2,-10);instruction(0,1,0,input);instruction(1,16,0,2);expect(2,OSG_EXPRESSION_OK,input<0?10:input>2?-10:10-input*10);
    }
    const int32_t bad_counts[]={-1,0,1,257};
    for(unsigned i=0;i<sizeof(bad_counts)/sizeof(*bad_counts);i++){
        instruction(1,16,0,bad_counts[i]);expect(2,OSG_EXPRESSION_MALFORMED,0);
    }
    instruction(1,16,3,2);expect(2,OSG_EXPRESSION_MALFORMED,0);
    instruction(1,16,1,2);expect(2,OSG_EXPRESSION_MALFORMED,0);
    instruction(1,16,0,2);point(1,0,20);expect(2,OSG_EXPRESSION_MALFORMED,0);
    point(1,-1,20);expect(2,OSG_EXPRESSION_MALFORMED,0);
    point(1,2,20);instruction(0,1,0,-1);
    for(unsigned fail=1;fail<=4;fail++){fail_read=fail;expect(2,OSG_EXPRESSION_READ,0);}fail_read=0;
    reset();for(unsigned i=0;i<256;i++)point(i,(int32_t)i,(int32_t)(i*2));
    instruction(0,1,0,200);instruction(1,16,0,256);expect(2,OSG_EXPRESSION_OK,400);assert(reads==258);
    /* An invalid late point must fail even when endpoint clamping selected
     * a value from the first point already. */
    instruction(0,1,0,-1);point(255,0,0);expect(2,OSG_EXPRESSION_MALFORMED,0);
}

#include "expression-vectors.h"

int main(void) {
    conversions();malformed();variables();arithmetic();lookups();golden_vectors();
    puts("Bounded C expressions: typed conversion, all ABI operations, stack/range/read faults, overflow and independent TS vectors passed");
    return 0;
}