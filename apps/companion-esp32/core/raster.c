#include "raster.h"
#include <string.h>
/* Original compact 3x5 uppercase diagnostic font, three bits per row.
 * Lowercase uses uppercase glyph shapes. Board adapters can replace text() with
 * their own font renderer without changing layout, input or publication IDs. */
static const uint8_t letters[][5] = {
 {2,5,7,5,5},{6,5,6,5,6},{3,4,4,4,3},{6,5,5,5,6},{7,4,6,4,7},
 {7,4,6,4,4},{3,4,5,5,3},{5,5,7,5,5},{7,2,2,2,7},{1,1,1,5,2},
 {5,5,6,5,5},{4,4,4,4,7},{5,7,7,5,5},{5,7,7,7,5},{2,5,5,5,2},
 {6,5,6,4,4},{2,5,5,3,1},{6,5,6,5,5},{3,4,2,1,6},{7,2,2,2,2},
 {5,5,5,5,7},{5,5,5,5,2},{5,5,7,7,5},{5,5,2,5,5},{5,5,2,2,2},{7,1,2,4,7}
};
static const uint8_t digits[][5] = {
 {7,5,5,5,7},{2,6,2,2,7},{6,1,2,4,7},{6,1,2,1,6},{5,5,7,1,1},
 {7,4,6,1,6},{3,4,6,5,2},{7,1,2,2,2},{2,5,2,5,2},{2,5,3,1,6}
};
static uint8_t glyph(char c, int row) {
    if (c >= 'a' && c <= 'z') c = (char)(c - 'a' + 'A');
    if (c >= 'A' && c <= 'Z') return letters[c - 'A'][row];
    if (c >= '0' && c <= '9') return digits[c - '0'][row];
    switch (c) {
        case ' ': return 0;
        case '.': return row == 4 ? 2 : 0;
        case ',': return row == 3 ? 2 : row == 4 ? 4 : 0;
        case ':': return row == 1 || row == 3 ? 2 : 0;
        case ';': return row == 1 || row == 3 ? 2 : row == 4 ? 4 : 0;
        case '-': return row == 2 ? 7 : 0;
        case '_': return row == 4 ? 7 : 0;
        case '+': return row == 2 ? 7 : row == 1 || row == 3 ? 2 : 0;
        case '=': return row == 1 || row == 3 ? 7 : 0;
        case '|': return 2;
        case '/': return row < 2 ? 1 : row == 2 ? 2 : 4;
        case '%': return row == 0 || row == 4 ? 5 : row == 2 ? 2 : 0;
        case '!': return row == 3 ? 0 : 2;
        case '(': return row == 0 || row == 4 ? 1 : 2;
        case ')': return row == 0 || row == 4 ? 4 : 2;
        default: return row == 0 ? 6 : row == 1 ? 1 : row == 2 || row == 4 ? 2 : 0;
    }
}
static void solid(void *ctx, int x, int y, int width, int height, uint16_t color) {
    osg_raster *r = ctx;
    if (r->failed || !r->write || width <= 0 || height <= 0) return;
    if(r->composing){
        int left=x>r->tile_x?x:r->tile_x,top=y>r->tile_y?y:r->tile_y;
        int right=x+width<r->tile_x+r->tile_width?x+width:r->tile_x+r->tile_width;
        int bottom=y+height<r->tile_y+r->tile_height?y+height:r->tile_y+r->tile_height;
        for(int row=top;row<bottom;row++)for(int col=left;col<right;col++)
            r->tile[(row-r->tile_y)*r->tile_width+col-r->tile_x]=color;
        return;
    }
    for (size_t i = 0; i < 256; i++) r->scratch[i] = color;
    for (int row = 0; row < height && !r->failed; row++) {
        for (int col = 0; col < width; col += 256) {
            const int n = width - col < 256 ? width - col : 256;
            if (!r->write(r->context, x + col, y + row, n, 1, r->scratch)) { r->failed = true; break; }
        }
    }
}
static void label(void *ctx, int x, int y, int width, int height, const char *s, uint16_t color) {
    if (width < 3 || height < 5 || !s || !*s) return;
    int scale = height / 5; if (scale > 3) scale = 3;
    const size_t n = strlen(s);
    while (scale > 1 && n * (size_t)(4 * scale) > (size_t)width) scale--;
    for (size_t i = 0; s[i]; i++) {
        const int dx = (int)i * 4 * scale;
        if (dx + 3 * scale > width) break;
        for (int row = 0; row < 5; row++) for (int col = 0; col < 3; col++)
            if (glyph(s[i], row) & (4 >> col)) solid(ctx, x + dx + col * scale, y + row * scale, scale, scale, color);
    }
}
osg_display osg_raster_display(osg_raster *r, uint16_t width, uint16_t height) {
    return (osg_display){ .context = r, .width = width, .height = height, .fill = solid, .text = label };
}
/* Compose each damaged region in bounded RGB565 tiles before the board sees
 * any pixels. Replaying the scene under a clip preserves overlap/z-order. */
static void region(osg_raster *r,osg_runtime *frame,int x,int y,int width,int height,uint64_t now){
    if(x<0){width+=x;x=0;}if(y<0){height+=y;y=0;}
    if(x+width>frame->display.width)width=frame->display.width-x;
    if(y+height>frame->display.height)height=frame->display.height-y;
    for(int row=y;row<y+height&&!r->failed;row+=8){
        for(int col=x;col<x+width&&!r->failed;col+=64){
            r->tile_x=col;r->tile_y=row;
            r->tile_width=x+width-col<64?x+width-col:64;
            r->tile_height=y+height-row<8?y+height-row:8;
            r->composing=true;
            osg_render(frame,now);
            r->composing=false;
            if(!r->write || !r->write(r->context,col,row,r->tile_width,r->tile_height,r->tile))r->failed=true;
        }
    }
}
void osg_raster_render(osg_raster *r,osg_runtime *runtime,osg_damage damage,uint64_t now){
    osg_display original=runtime->display;
    runtime->display=osg_raster_display(r,original.width,original.height);
    if(damage.full)region(r,runtime,0,0,original.width,original.height,now);
    else for(size_t i=0;i<runtime->panel->widget_count&&!r->failed;i++){
        if(!(damage.widgets&(UINT32_C(1)<<i)))continue;
        int x,y,width,height;osg_widget_rect(runtime,i,&x,&y,&width,&height);
        region(r,runtime,x,y,width,height,now);
    }
    runtime->display=original;
}
