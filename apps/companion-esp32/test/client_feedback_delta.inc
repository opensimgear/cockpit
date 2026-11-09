/* Included by client_test.c so this focused transport case can use its private
 * live-session harness without adding another binary or duplicate fixtures. */
static cJSON *serial_feedback_record(const char *link,const char *sequence,const char *binding_generation,const char *mode,double value){
    cJSON *record=cJSON_CreateObject(),*frame=cJSON_CreateObject(),*feedback=cJSON_CreateObject(),*values=cJSON_CreateArray(),*sample=cJSON_CreateObject();assert(record&&frame&&feedback&&values&&sample);
    cJSON_AddStringToObject(record,"command","serial-feedback");cJSON_AddStringToObject(record,"linkId",link);
    cJSON_AddBoolToObject(frame,"ok",true);cJSON_AddStringToObject(frame,"type","feedback-event");
    cJSON_AddNumberToObject(feedback,"version",1);cJSON_AddStringToObject(feedback,"type","feedback");
    cJSON_AddStringToObject(feedback,"clientId",session.client);cJSON_AddStringToObject(feedback,"sessionId",session.session);
    cJSON_AddStringToObject(feedback,"generation",session.generation);cJSON_AddStringToObject(feedback,"publicationHash",session.publication);
    cJSON_AddStringToObject(feedback,"controllerInstanceId",session.instance);cJSON_AddStringToObject(feedback,"sequence",sequence);
    cJSON_AddStringToObject(feedback,"bindingGeneration",binding_generation);cJSON_AddStringToObject(feedback,"mode",mode);
    if(!strcmp(mode,"snapshot")){
        cJSON *bindings=cJSON_CreateArray(),*binding=cJSON_CreateObject();assert(bindings&&binding);
        cJSON_AddStringToObject(binding,"channelId","speed-channel");cJSON_AddStringToObject(binding,"outputId","speed");
        cJSON_AddItemToArray(bindings,binding);cJSON_AddItemToObject(feedback,"bindings",bindings);
    }
    cJSON_AddStringToObject(sample,"channelId","speed-channel");cJSON_AddStringToObject(sample,"outputId","speed");
    cJSON_AddStringToObject(sample,"status","fresh");cJSON_AddNumberToObject(sample,"value",value);
    cJSON_AddStringToObject(sample,"provenance","game-confirmed");cJSON_AddStringToObject(sample,"sourceKind","simulator");
    cJSON_AddNumberToObject(sample,"sourceAgeMs",0);cJSON_AddStringToObject(sample,"unit","kt");
    cJSON *range=cJSON_CreateObject();assert(range);cJSON_AddNumberToObject(range,"min",0);cJSON_AddNumberToObject(range,"max",600);cJSON_AddItemToObject(sample,"range",range);
    cJSON_AddItemToArray(values,sample);cJSON_AddItemToObject(feedback,"values",values);
    cJSON_AddItemToObject(frame,"feedback",feedback);cJSON_AddItemToObject(record,"frame",frame);return record;
}
static void serial_feedback_tests(void){
    live_ready();connection.direct=true;osg_copy(connection.link,sizeof(connection.link),"feedback-link");
    cJSON *event=serial_feedback_record("wrong-link","1","1","snapshot",125.5);
    const uint64_t lease=input.last_ack_ms;test_dirty=false;osg_network_serial_feedback(event);
    assert(!test_dirty&&!session.feedback_stream);
    cJSON_ReplaceItemInObjectCaseSensitive(event,"linkId",cJSON_CreateString(connection.link));osg_network_serial_feedback(event);
    assert(test_dirty&&session.feedback_stream&&session.feedback_sequence==1&&input.last_ack_ms==lease&&!remote_armed);
    assert(test_runtime.samples[3].number==125.5);
    cJSON_Delete(event);event=serial_feedback_record(connection.link,"2","1","delta",200);
    test_dirty=false;osg_network_serial_feedback(event);
    assert(test_dirty&&session.feedback_sequence==2&&test_runtime.samples[3].number==200);
    const uint64_t delta_received=test_runtime.samples[3].received_ms;
    cJSON_Delete(event);event=serial_feedback_record(connection.link,"2","1","delta",300);
    test_dirty=false;osg_network_serial_feedback(event);
    assert(!test_dirty&&test_runtime.samples[3].number==200&&test_runtime.samples[3].received_ms==delta_received);
    cJSON_Delete(event);event=serial_feedback_record(connection.link,"1","1","delta",300);
    test_dirty=false;osg_network_serial_feedback(event);
    assert(!test_dirty&&session.feedback_sequence==2&&test_runtime.samples[3].number==200);
    cJSON_Delete(event);event=serial_feedback_record(connection.link,"3","2","delta",300);
    test_dirty=false;osg_network_serial_feedback(event);
    assert(!test_dirty&&session.feedback_sequence==2&&test_runtime.samples[3].number==200);

    live_ready();permit_inputs=true;live_arm();fake_ms++;osg_network_touch(true,true,100,100);advance();assert(mock_button);
    connection.direct=true;osg_copy(connection.link,sizeof(connection.link),"feedback-link");
    cJSON *removed=cJSON_Parse("{\"linkId\":\"feedback-link\",\"frame\":{\"ok\":true,\"type\":\"assignments-event\",\"assignments\":[]}}");
    osg_network_serial_feedback(removed);assert(state==LIVE);usb_assignments();
    assert(state==AUTH&&!remote_armed&&!mock_host_armed&&!mock_button);permit_inputs=false;cJSON_Delete(removed);
    live_ready();connection.direct=false;connection.ws=(void *)1;
    mock_wifi_message=cJSON_Duplicate(cJSON_GetObjectItemCaseSensitive(event,"frame"),true);
    const uint64_t wifi_lease=input.last_ack_ms;test_dirty=false;wifi_feedback();
    assert(!test_dirty&&input.last_ack_ms==wifi_lease);cJSON_Delete(event);
    mock_wifi_message=cJSON_Parse("{\"ok\":true,\"type\":\"assignments-event\",\"assignments\":[]}");wifi_feedback();
    assert(state==AUTH&&!remote_armed);stop();
}

static void deliver_feedback(bool direct,const cJSON *event){
    if(direct){
        osg_network_serial_feedback(event);
    }else{
        assert(!mock_wifi_message);
        mock_wifi_message=cJSON_Duplicate(cJSON_GetObjectItemCaseSensitive(event,"frame"),true);
        assert(mock_wifi_message);
        wifi_feedback();
    }
}

static void feedback_session_replacement(bool direct){
    live_ready_for_transport(direct);
    if(direct)osg_copy(connection.link,sizeof(connection.link),"retired-link");
    cJSON *retired=serial_feedback_record(connection.link,"1","1","snapshot",125.5);
    deliver_feedback(direct,retired);
    assert(session.feedback_envelope&&test_runtime.samples[3].available);
    assert(test_runtime.samples[3].number==125.5);

    stop();
    assert(!test_runtime.connected&&!test_runtime.samples[3].available&&!remote_armed);
    live_ready_for_transport(direct);
    if(direct)osg_copy(connection.link,sizeof(connection.link),"replacement-link");
    const cJSON *retired_frame=cJSON_GetObjectItemCaseSensitive(retired,"frame");
    const cJSON *retired_feedback=cJSON_GetObjectItemCaseSensitive(retired_frame,"feedback");
    assert(strcmp(osg_string(retired_feedback,"sessionId"),session.session));
    const uint64_t lease=input.last_ack_ms;
    test_dirty=false;
    deliver_feedback(direct,retired);
    if(direct){
        /* Both link identity and the inner session must fence delayed USB data. */
        cJSON_ReplaceItemInObjectCaseSensitive(retired,"linkId",cJSON_CreateString(connection.link));
        deliver_feedback(direct,retired);
    }
    cJSON_Delete(retired);
    assert(!test_dirty&&!session.feedback_envelope&&!test_runtime.samples[3].available);

    cJSON *delta=serial_feedback_record(connection.link,"2","1","delta",200);
    deliver_feedback(direct,delta);
    assert(!test_dirty&&!session.feedback_envelope&&!test_runtime.samples[3].available);
    cJSON *baseline=serial_feedback_record(connection.link,"1","1","snapshot",150);
    deliver_feedback(direct,baseline);
    assert(test_dirty&&session.feedback_envelope&&test_runtime.samples[3].number==150);
    cJSON_Delete(baseline);
    test_dirty=false;
    deliver_feedback(direct,delta);
    cJSON_Delete(delta);
    assert(test_dirty&&session.feedback_sequence==2&&test_runtime.samples[3].number==200);
    assert(input.last_ack_ms==lease&&!remote_armed&&!test_runtime.armed);
    stop();
}
