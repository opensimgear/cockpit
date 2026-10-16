#ifndef OSG_SUNTON_TOUCH_H
#define OSG_SUNTON_TOUCH_H
#include <stdbool.h>
static inline bool osg_sunton_point(int rx,int ry,int xmin,int xmax,int ymin,int ymax,int *x,int *y){
    if(xmin<0||ymin<0||xmax>4095||ymax>4095||xmin>=xmax||ymin>=ymax||rx<=0||rx>=4095||ry<=0||ry>=4095)return false;
    int px=(ymax-ry)*479/(ymax-ymin),py=(rx-xmin)*319/(xmax-xmin);
    *x=px<0?0:px>479?479:px;*y=py<0?0:py>319?319:py;return true;
}
#endif
