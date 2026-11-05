static cJSON *pairing_reply(void){
    cJSON *reply=cJSON_CreateObject();assert(reply);
    cJSON_AddBoolToObject(reply,"ok",true);cJSON_AddStringToObject(reply,"type","paired");
    cJSON_AddStringToObject(reply,"requestId","request");cJSON_AddStringToObject(reply,"hostId","host");
    cJSON_AddStringToObject(reply,"companionId",settings.companion);
    cJSON_AddStringToObject(reply,"token","AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");return reply;
}
static void approve_pairing(void){
    if(!connection.direct){mock_wifi_message=pairing_reply();wifi_feedback();return;}
    cJSON *event=cJSON_CreateObject();cJSON_AddStringToObject(event,"linkId",connection.link);
    cJSON_AddItemToObject(event,"frame",pairing_reply());osg_network_serial_feedback(event);cJSON_Delete(event);
}
static void pairing_notification_tests(void){
    setup();advance();assert(state==PAIR_WAIT&&mock_pairing_running);
    assert(!connection.ws);const int opens=socket_opens;
    const int before=requests;
    for(unsigned i=0;i<20;i++){fake_ms+=400;advance();wifi_feedback();assert(state==PAIR_WAIT&&requests==before);}
    cJSON *reply=pairing_reply();pairing_accept(reply);cJSON_Delete(reply);
    assert(state==AUTH&&!mock_pairing_running&&!settings.secret[0]&&!remote_armed);
    advance();assert(state==OPEN&&socket_opens==opens+1);
    setup();advance();fake_ms=pair_deadline;advance();assert(state==FAULT&&!settings.secret[0]);
    for(unsigned invalid=0;invalid<4;invalid++){
        setup();advance();reply=pairing_reply();
        const char *field=invalid==0?"requestId":invalid==1?"hostId":invalid==2?"companionId":"token";
        cJSON_ReplaceItemInObjectCaseSensitive(reply,field,cJSON_CreateString("wrong"));
        pairing_accept(reply);cJSON_Delete(reply);assert(state==FAULT&&!settings.token[0]&&!settings.secret[0]);
    }
    setup();mock_pairing_failure=true;advance();assert(state==FAULT);
    setup();advance();mock_pairing_running=false;wifi_feedback();assert(state==FAULT);
    setup();advance();stop();reply=pairing_reply();pairing_accept(reply);cJSON_Delete(reply);assert(state==OFF&&!settings.token[0]);
    setup();settings.direct=true;connection.direct=true;strcpy(connection.link,"current-link");advance();assert(state==PAIR_WAIT&&!strcmp(requested_type,"pair-subscribe"));
    cJSON *event=cJSON_CreateObject();cJSON_AddStringToObject(event,"linkId","old-link");cJSON_AddItemToObject(event,"frame",pairing_reply());
    osg_network_serial_feedback(event);assert(state==PAIR_WAIT&&!settings.token[0]);
    cJSON_ReplaceItemInObjectCaseSensitive(event,"linkId",cJSON_CreateString(connection.link));
    osg_network_serial_feedback(event);assert(state==AUTH&&!settings.secret[0]&&!remote_armed);
    cJSON_Delete(event);stop();
}
