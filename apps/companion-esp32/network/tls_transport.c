#include "platform.h"
#include "diagnostics.h"
#include "esp_transport_ssl.h"
#include "esp_transport_ws.h"
#include "esp_timer.h"
#include "mbedtls/base64.h"
#include "mbedtls/sha256.h"
#include "mbedtls/x509_crt.h"
#include "mbedtls/platform_util.h"
#include <string.h>
#include <stdio.h>
#include <time.h>

static bool sha(const void *bytes,size_t n,char out[72]) {
    unsigned char digest[32];if(mbedtls_sha256(bytes,n,digest,0))return false;
    memcpy(out,"sha256:",7);for(size_t i=0;i<32;i++)(void)snprintf(out+7+i*2,3,"%02x",digest[i]);return true;
}
static bool base64(const char *s,unsigned char *out,size_t cap,size_t *n){
    if(!s)return false;
    return mbedtls_base64_decode(out,cap,n,(const unsigned char *)s,strlen(s))==0;
}
const osg_crypto osg_platform_crypto={sha,base64};
bool osg_trusted_ca(const osg_settings *s,unsigned char *der,size_t capacity,size_t *n){
    if(!base64(s->ca,der,capacity,n))return false;
    mbedtls_x509_crt crt;mbedtls_x509_crt_init(&crt);char pin[72];
    bool ok=mbedtls_x509_crt_parse_der(&crt,der,*n)==0&&crt.next==NULL&&crt.raw.len==*n&&mbedtls_x509_crt_get_ca_istrue(&crt)&&
        !mbedtls_x509_time_is_future(&crt.valid_from)&&!mbedtls_x509_time_is_past(&crt.valid_to)&&sha(crt.pk_raw.p,crt.pk_raw.len,pin);
    if(ok){unsigned diff=0;for(size_t i=0;i<72;i++)diff|=(unsigned char)pin[i]^(unsigned char)s->pin[i];ok=diff==0;}
    mbedtls_x509_crt_free(&crt);return ok;
}
bool osg_socket_open(osg_socket *socket,const osg_settings *s){
    if(s->direct)return osg_serial_open(socket);
    char host[254];int port;memset(socket,0,sizeof(*socket));
    if(time(NULL)<1735689600||!osg_endpoint(s->endpoint,host,&port)||!osg_trusted_ca(s,socket->ca_der,sizeof(socket->ca_der),&socket->ca_bytes))return false;
    socket->tls=esp_transport_ssl_init();if(!socket->tls)return false;
    esp_transport_ssl_set_cert_data_der(socket->tls,(const char *)socket->ca_der,(int)socket->ca_bytes);
    /* No global/public trust store, CN bypass, redirects or plaintext fallback. */
    socket->ws=esp_transport_ws_init(socket->tls);
    if(!socket->ws){osg_socket_close(socket);return false;}
    const esp_transport_ws_config_t config={.ws_path="/companion",.sub_protocol="opensimgear-companion-v1",.propagate_control_frames=true};
    if(esp_transport_ws_set_config(socket->ws,&config)!=ESP_OK||esp_transport_connect(socket->ws,host,port,3000)!=0||esp_transport_ws_get_upgrade_request_status(socket->ws)!=101){osg_socket_close(socket);return false;}
    return true;
}
void osg_socket_close(osg_socket *s){
    if(s->direct){osg_serial_close(s);mbedtls_platform_zeroize(s,sizeof(*s));return;}
        if(s->ws){esp_transport_close(s->ws);esp_transport_destroy(s->ws);}
    if(s->tls)esp_transport_destroy(s->tls);
    mbedtls_platform_zeroize(s,sizeof(*s));
}
char osg_transport_frame[OSG_FRAME_BYTES+1], osg_transport_reply[OSG_REPLY_BYTES];
static bool receive_frame(osg_socket *s,size_t *length,int64_t deadline){
    size_t used=0,frame_used=0;int payload=0;bool fragmented=false;
    for(;;){
        int64_t remaining=(deadline-esp_timer_get_time())/1000;if(remaining<=0)return false;
        const int n=esp_transport_read(s->ws,osg_transport_frame+used,(int)(OSG_FRAME_BYTES-used),(int)remaining);
        if(n<0)return false;
        osg_diagnostics_bytes(OSG_DIAG_WS_RX,(size_t)n);
        const int opcode=esp_transport_ws_get_read_opcode(s->ws);
        if(!n&&opcode==WS_TRANSPORT_OPCODES_NONE)continue;
        const int current=esp_transport_ws_get_read_payload_len(s->ws);
        if(!frame_used){payload=current;if(payload<0||payload>(int)(OSG_FRAME_BYTES-used))return false;}
        if(opcode==WS_TRANSPORT_OPCODES_CLOSE)return false;
        if(opcode==WS_TRANSPORT_OPCODES_PING||opcode==WS_TRANSPORT_OPCODES_PONG){
            if(current>125||n!=current)return false;
            if(opcode==WS_TRANSPORT_OPCODES_PING&&esp_transport_ws_send_raw(s->ws,WS_TRANSPORT_OPCODES_PONG|WS_TRANSPORT_OPCODES_FIN,osg_transport_frame+used,n,200)!=n)return false;
            frame_used=0;continue;
        }
        if(opcode!=WS_TRANSPORT_OPCODES_TEXT&&opcode!=WS_TRANSPORT_OPCODES_CONT)return false;
        if(!frame_used&&((fragmented&&opcode!=WS_TRANSPORT_OPCODES_CONT)||(!fragmented&&opcode!=WS_TRANSPORT_OPCODES_TEXT)))return false;
        used+=(size_t)n;frame_used+=(size_t)n;
        if(frame_used>(size_t)payload||used>OSG_FRAME_BYTES)return false;
        if(frame_used==(size_t)payload){
            if(esp_transport_ws_get_fin_flag(s->ws)){osg_transport_frame[used]=0;*length=used;return used>0;}
            fragmented=true;frame_used=0;
        }
        if(used==OSG_FRAME_BYTES)return false;
    }
}
bool osg_socket_request(osg_socket *s,cJSON *request,cJSON **response,unsigned timeout){
    if(s->direct)return osg_serial_request(s,request,response,timeout);
    char outbound[4096];*response=NULL;
    const bool encoded=request&&cJSON_PrintPreallocated(request,outbound,sizeof(outbound),false);cJSON_Delete(request);
    if(!encoded){mbedtls_platform_zeroize(outbound,sizeof(outbound));return false;}
    const size_t bytes=strlen(outbound);
    const int sent=esp_transport_ws_send_raw(s->ws,WS_TRANSPORT_OPCODES_TEXT|WS_TRANSPORT_OPCODES_FIN,outbound,(int)bytes,500);
    if(sent>0)osg_diagnostics_bytes(OSG_DIAG_WS_TX,(size_t)sent);
    mbedtls_platform_zeroize(outbound,sizeof(outbound));if(sent!=(int)bytes)return false;
    mbedtls_platform_zeroize(osg_transport_reply,sizeof(osg_transport_reply));
    const int64_t deadline=esp_timer_get_time()+(int64_t)timeout*1000;
    osg_transfer transfer={.bytes=osg_transport_reply,.capacity=sizeof(osg_transport_reply)};
    for(;;){size_t n=0;if(!receive_frame(s,&n,deadline)){mbedtls_platform_zeroize(osg_transport_frame,sizeof(osg_transport_frame));mbedtls_platform_zeroize(osg_transport_reply,sizeof(osg_transport_reply));return false;}
        const int result=osg_reply_feed(&transfer,osg_transport_frame,n,osg_platform_crypto,response);
        mbedtls_platform_zeroize(osg_transport_frame,n);
        if(result<0){mbedtls_platform_zeroize(osg_transport_reply,sizeof(osg_transport_reply));return false;}
        if(result==1){mbedtls_platform_zeroize(osg_transport_reply,sizeof(osg_transport_reply));return cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(*response,"ok"));}
    }
}
