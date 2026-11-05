/* The network task owns pairing state. Transport workers only retain one
 * terminal reply; observing approval never authenticates or arms the device. */
static bool pairing_subscribe(void){
    request_label="Pairing approval";
    if(!connection.direct)return osg_wifi_pairing_start(&settings,pending)||
        request_problem("Cannot start pairing notification transport");
    cJSON *v=message("pair-subscribe"),*reply=NULL;
    if(!v)return request_problem("Cannot allocate pairing subscription");
    cJSON_AddStringToObject(v,"requestId",pending);
    cJSON_AddStringToObject(v,"secret",settings.secret);
    bool ok=request(v,&reply,3000)&&cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(reply,"ok"))&&
        equal(osg_string(reply,"type"),"pair-subscribed");
    cJSON_Delete(reply);return ok;
}

static void pairing_accept(const cJSON *reply){
    if(state!=PAIR_WAIT)return;
    request_label="Pairing approval";
    if(millis()>=pair_deadline){fail("pairing-approval-expired");return;}
    bool ok=cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(reply,"ok"))&&
        equal(osg_string(reply,"requestId"),pending)&&identity(reply,"paired")&&
        osg_secret(osg_string(reply,"token"));
    if(!ok){
        request_problem("Pairing subscription ended or returned an invalid approval");
        fail_request();return;
    }
    osg_copy(settings.token,sizeof(settings.token),osg_string(reply,"token"));
    mbedtls_platform_zeroize(settings.secret,sizeof(settings.secret));
    osg_wifi_feedback_stop();pending[0]=0;
    if(settings.persist&&!osg_store_save(&settings)){fail("setup-save-failed-repair-required");return;}
    installation_saved=settings.persist;state=AUTH;next_request=millis();report("paired");
}

static void pairing_begin(void){
    if(millis()>=pair_deadline){fail("pairing-offer-expired");return;}
    if(!open_socket()){fail("host-tls-or-connection-failed");return;}
    cJSON *v=message("pair"),*reply=NULL;
    cJSON_AddStringToObject(v,"secret",settings.secret);cJSON_AddStringToObject(v,"companionId",settings.companion);cJSON_AddStringToObject(v,"name",settings.name);
    bool ok=request(v,&reply,3000)&&equal(osg_string(reply,"type"),"pair-pending")&&
        osg_stable_id(osg_string(reply,"requestId"))&&osg_copy(pending,sizeof(pending),osg_string(reply,"requestId"));
    if(ok){
        // Approval owns a separate Wi-Fi socket. Reopen primary for AUTH after
        // an arbitrarily quiet wait rather than reusing an expired request link.
        if(!connection.direct)osg_socket_close(&connection);
        state=PAIR_WAIT;next_request=millis();ok=pairing_subscribe();
        if(ok&&state==PAIR_WAIT)report("approve-pairing-in-cockpit");
    }
    cJSON_Delete(reply);if(!ok)fail_request();
}
