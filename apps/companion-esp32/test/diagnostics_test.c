#include "../core/diagnostics.c"
#include <assert.h>
#include <stdio.h>
int main(void) {
    osg_diagnostics d;
    osg_diagnostics_reset(); osg_diagnostics_snapshot(NULL); osg_diagnostics_snapshot(&d);
    assert(!d.metrics[0].count && !d.memory_samples);
    for (unsigned i=0;i<OSG_DIAG_METRIC_COUNT;i++) assert(osg_diagnostics_metric_name((osg_diag_metric)i));
    assert(!osg_diagnostics_metric_name(OSG_DIAG_METRIC_COUNT));
    osg_diagnostics_record(OSG_DIAG_METRIC_COUNT,1,1,true);
    osg_diagnostics_bytes(OSG_DIAG_BYTE_COUNT,1);
    osg_diagnostics_stack(OSG_DIAG_TASK_COUNT,1);
    for (unsigned i=0;i<100;i++) osg_diagnostics_record(OSG_DIAG_RENDER,i,2,i!=99);
    osg_diagnostics_record(OSG_DIAG_PRESENT,UINT64_MAX,3,false);
    osg_diagnostics_record(OSG_DIAG_PRESENT,1,3,true);
    osg_diagnostics_record(OSG_DIAG_PRESENT,UINT64_MAX,3,true);
    osg_diagnostics_bytes(OSG_DIAG_SERIAL_TX,SIZE_MAX);
    osg_diagnostics_bytes(OSG_DIAG_SERIAL_TX,1);
    osg_diagnostics_memory(100,90,80); osg_diagnostics_memory(120,80,100); osg_diagnostics_memory(95,70,60);
    osg_diagnostics_stack(OSG_DIAG_MAIN_STACK,100); osg_diagnostics_stack(OSG_DIAG_MAIN_STACK,120); osg_diagnostics_stack(OSG_DIAG_MAIN_STACK,90);
    osg_diagnostics_health(3,10,20,30,40); osg_diagnostics_health(2,5,6,7,8);
    osg_diagnostics_health(SIZE_MAX,5,6,7,8);
    osg_diagnostics_snapshot(&d);
    const osg_diag_stat *s=&d.metrics[OSG_DIAG_RENDER];
    assert(s->count==100&&s->failed==1&&s->bytes==200&&s->total_us==4950&&s->max_us==99);
    assert(osg_diagnostics_percentile(s,1)==1&&osg_diagnostics_percentile(s,50)==64);
    assert(osg_diagnostics_percentile(s,95)==128&&osg_diagnostics_percentile(s,99)==128);
    assert(d.metrics[OSG_DIAG_PRESENT].total_us==UINT64_MAX&&d.metrics[OSG_DIAG_PRESENT].max_us==UINT64_MAX);
    assert(osg_diagnostics_percentile(&d.metrics[OSG_DIAG_PRESENT],100)==UINT64_MAX);
    assert(!osg_diagnostics_percentile(NULL,50)&&!osg_diagnostics_percentile(s,0)&&!osg_diagnostics_percentile(s,101));
    assert(!osg_diagnostics_percentile(&d.metrics[OSG_DIAG_TOUCH_SPI],50));
    osg_diag_stat corrupt={.count=1};assert(osg_diagnostics_percentile(&corrupt,50)==UINT64_MAX);
    assert(d.memory_samples==3&&d.free_internal_bytes==95&&d.minimum_internal_bytes==70&&d.largest_internal_bytes==60&&d.smallest_largest_internal_bytes==60);
    assert(d.stack_samples[0]==3&&d.stack_min_bytes[0]==90);
    assert(d.queue_max==UINT32_MAX&&d.pending_input_max_us==10&&d.touch_age_max_us==20&&d.ack_age_max_us==30&&d.render_age_max_us==40);
    assert(d.bytes[OSG_DIAG_SERIAL_TX]>=SIZE_MAX);
    /* Simulate counter exhaustion without billions of calls. Discard complete
     * samples after count saturation so quantiles never use mismatched totals. */
    counters.metrics[OSG_DIAG_RENDER].count=UINT32_MAX;
    osg_diagnostics_record(OSG_DIAG_RENDER,0,0,true);
    assert(counters.metrics[OSG_DIAG_RENDER].dropped==1);
    counters.metrics[OSG_DIAG_RENDER].dropped=UINT32_MAX;osg_diagnostics_record(OSG_DIAG_RENDER,0,0,true);
    counters.memory_samples=UINT32_MAX;osg_diagnostics_memory(1,1,1);assert(counters.memory_samples==UINT32_MAX);
    counters.stack_samples[0]=UINT32_MAX;osg_diagnostics_stack(OSG_DIAG_MAIN_STACK,1);assert(counters.stack_samples[0]==UINT32_MAX);
    osg_diagnostics_reset();osg_diagnostics_snapshot(&d);assert(!d.metrics[OSG_DIAG_RENDER].count&&!d.queue_max&&!d.bytes[0]);
    puts("Bounded diagnostics histogram, percentile bounds, saturation, memory/stack and lease-age tests passed");
}
