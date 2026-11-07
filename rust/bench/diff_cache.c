#include "lcms2.h"
#include <stdio.h>
int main(void) {
    cmsHPROFILE in = cmsOpenProfileFromFile("test5.icc","r");
    cmsHPROFILE out = cmsOpenProfileFromFile("test3.icc","r");
    cmsHTRANSFORM h = cmsCreateTransform(in, TYPE_RGBA_16, out, TYPE_RGBA_16, INTENT_PERCEPTUAL, 0);
    unsigned short ib[4], ob[4];
    for (int r=0;r<65536;r+=997) for (int g=0;g<65536;g+=4099) for (int b=0;b<65536;b+=4099) {
        ib[0]=r;ib[1]=g;ib[2]=b;ib[3]=0x8000;
        cmsDoTransform(h,ib,ob,1);
        printf("%u %u %u -> %u %u %u %u\n",ib[0],ib[1],ib[2],ob[0],ob[1],ob[2],ob[3]);
    }
    return 0;
}
