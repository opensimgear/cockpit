#include "../network/wifi_feedback.c"
#include <assert.h>
#include <stdio.h>

static const char *frames[8];
static const char payload[]="{\"ok\":true,\"type\":\"feedback-event\"}";
static const char hash[]="sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
static bool task_failure,idle_once,open_failure,write_failure,read_failure;
static size_t index;
static cJSON *delivered;
static void (*worker)(void *);
size_t osg_transport_reply_capacity=OSG_REPLY_BYTES;

static bool sha(const void *bytes,size_t size,char out[72]){
    if(size!=strlen(payload)||memcmp(bytes,payload,size))return false;
    memcpy(out,hash,sizeof(hash));return true;
}
static bool decode(const char *text,unsigned char *out,size_t capacity,size_t *size){
    if(strcmp(text,"fixture")||capacity<strlen(payload))return false;
    *size=strlen(payload);memcpy(out,payload,*size);return true;
}
const osg_crypto osg_platform_crypto={sha,decode};

SemaphoreHandle_t xSemaphoreCreateBinary(void){return &index;}
int xSemaphoreTake(SemaphoreHandle_t semaphore,unsigned timeout){
    (void)timeout;assert(semaphore==&index);
    cJSON *message=osg_wifi_feedback_take();
    if(message){assert(!delivered);delivered=message;}
    return 1;
}
int xSemaphoreGive(SemaphoreHandle_t semaphore){assert(semaphore==&index);return 1;}
TaskHandle_t xTaskGetCurrentTaskHandle(void){return &index;}
void xTaskNotifyGive(TaskHandle_t task){assert(task==&index);}
void vTaskDelete(TaskHandle_t task){assert(!task);}
int xTaskCreate(void (*fn)(void *),const char *name,unsigned stack,void *arg,unsigned priority,void *task){
    (void)name;(void)stack;(void)arg;(void)priority;(void)task;worker=fn;return task_failure?0:pdPASS;
}
void mbedtls_platform_zeroize(void *bytes,size_t size){memset(bytes,0,size);}
int64_t esp_timer_get_time(void){return 0;}
bool osg_socket_open(osg_socket *socket,const osg_settings *settings){
    assert(strcmp(settings->endpoint,"wss://test:1234/companion")==0);
    if(open_failure)return false;
    socket->ws=&index;return true;
}
void osg_socket_close(osg_socket *socket){socket->ws=NULL;}
int esp_transport_poll_read(esp_transport_handle_t handle,int timeout){
    assert(handle==&index&&timeout==500);
    if(idle_once){idle_once=false;return 0;}
    return frames[index]?1:-1;
}
bool osg_socket_write_text(osg_socket *socket,const char *data,size_t size,unsigned timeout){
    assert(socket->ws&&timeout==500&&size==strlen(data)&&strstr(data,assignments_only?"assignments-subscribe":"esp32-feedback-subscribe"));
    return !write_failure;
}
bool osg_socket_read_text(osg_socket *socket,char *out,size_t capacity,size_t *size,int64_t deadline){
    assert(socket->ws&&deadline>0);
    if(read_failure)return false;
    *size=strlen(frames[index]);assert(*size<=capacity);
    memcpy(out,frames[index++],*size+1);return true;
}

int main(void){
    osg_json_init();
    const osg_settings settings={.endpoint="wss://test:1234/companion",.token="abcdefghijklmnopqrstuvwxyz0123456789abcdefg"};
    const osg_live_session session={.session="session",.generation="1",
        .publication="sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"};
    assert(osg_wifi_feedback_start(&settings,&session,NULL));
    assert(!osg_wifi_feedback_start(&settings,&session,NULL));
    assert(!osg_wifi_feedback_take());
    frames[0]="{\"ok\":true,\"type\":\"feedback-subscribed\"}";
    frames[1]="{\"ok\":true,\"type\":\"feedback-event\"}";
    worker(NULL);
    assert(!osg_wifi_feedback_running());
    assert(delivered&&strcmp(osg_string(delivered,"type"),"feedback-event")==0);
    cJSON_Delete(delivered);delivered=NULL;
    assert(osg_wifi_feedback_start(&settings,&session,NULL));
    frames[index]="{\"ok\":false,\"type\":\"error\"}";
    worker(NULL);
    assert(!osg_wifi_feedback_running()&&!osg_wifi_feedback_take()&&!delivered);
    assert(osg_wifi_feedback_start(&settings,&session,NULL));
    osg_wifi_feedback_stop();
    worker(NULL);
    assert(!osg_wifi_feedback_running());
    task_failure=true;
    assert(!osg_wifi_feedback_start(&settings,&session,NULL));
    task_failure=false;
    char start[256];
    snprintf(start,sizeof(start),"{\"type\":\"transfer-start\",\"id\":\"feedback\",\"bytes\":%zu,\"sha256\":\"%s\"}",strlen(payload),hash);
    index=0;idle_once=true;
    frames[0]="{\"ok\":true,\"type\":\"feedback-subscribed\"}";
    frames[1]=start;
    frames[2]="{\"type\":\"transfer-chunk\",\"id\":\"feedback\",\"index\":0,\"base64\":\"fixture\"}";
    frames[3]="{\"type\":\"transfer-end\",\"id\":\"feedback\"}";
    assert(osg_wifi_feedback_start(&settings,&session,NULL));
    worker(NULL);
    assert(delivered&&strcmp(osg_string(delivered,"type"),"feedback-event")==0);
    cJSON_Delete(delivered);
    delivered=NULL;
    for(unsigned failure=0;failure<4;failure++){
        open_failure=failure==0;
        write_failure=failure==1;
        read_failure=failure==2;
        index=0;frames[0]="{\"type\":\"transfer-start\",\"bytes\":0}";
        assert(osg_wifi_feedback_start(&settings,&session,NULL));
        worker(NULL);
        assert(!osg_wifi_feedback_running()&&!osg_wifi_feedback_take()&&!delivered);
    }
    assert(!osg_wifi_feedback_start(&settings,&(osg_live_session){0},NULL));
    open_failure=write_failure=read_failure=false;index=0;
    memset(frames,0,sizeof(frames));
    frames[0]="{\"ok\":true,\"type\":\"assignments-subscribed\"}";
    frames[1]="{\"ok\":true,\"type\":\"assignments-event\",\"assignments\":[]}";
    assert(!osg_wifi_feedback_start(&settings,NULL,"bad id"));
    assert(osg_wifi_feedback_start(&settings,NULL,"wifi-primary"));
    worker(NULL);
    assert(delivered&&strcmp(osg_string(delivered,"type"),"assignments-event")==0);
    cJSON_Delete(delivered);delivered=NULL;
    index=0;frames[0]="{\"ok\":true,\"type\":\"feedback-subscribed\"}";
    assert(osg_wifi_feedback_start(&settings,NULL,"wifi-primary"));
    worker(NULL);
    assert(!osg_wifi_feedback_running()&&!delivered);
    assert(!osg_wifi_feedback_start(&(osg_settings){0},NULL,"wifi-primary"));
    free(buffer);buffer=NULL;
    assert(osg_json_allocated()==0);
}
