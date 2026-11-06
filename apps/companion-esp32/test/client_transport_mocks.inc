static cJSON *mock_pending_command;
cJSON *osg_serial_take_command(void){cJSON *v=mock_pending_command;mock_pending_command=NULL;return v;}
cJSON *osg_wifi_scan(void){return cJSON_CreateArray();}
bool osg_serial_start(void){return true;}
bool osg_serial_pending(void){return false;}
static bool serial_wake=true;
bool osg_serial_has_wake(void){return serial_wake;}
bool osg_wifi_feedback_start(const osg_settings *s,const osg_live_session *l,const char *id){(void)s;(void)l;(void)id;return false;}
static bool mock_pairing_running,mock_pairing_failure;
bool osg_wifi_pairing_start(const osg_settings *s,const char *id){assert(osg_secret(s->secret)&&!strcmp(id,"request"));mock_pairing_running=!mock_pairing_failure;return mock_pairing_running;}
void osg_wifi_feedback_stop(void){mock_pairing_running=false;}
unsigned ulTaskNotifyTake(int clear,unsigned wait){(void)clear;vTaskDelay(wait);return 0;}
bool osg_wifi_feedback_running(void){return mock_pairing_running;}
static cJSON *mock_wifi_message;
static bool mock_serial_assignments;
cJSON *osg_wifi_feedback_take(void){cJSON *message=mock_wifi_message;mock_wifi_message=NULL;return message;}
void osg_serial_receive_feedback(const osg_socket *socket,cJSON *record){(void)socket;osg_network_serial_feedback(record);}
