#include "lcms2.h"
#include <stdio.h>
int main(void) {
    cmsToneCurve* g = cmsBuildGamma(NULL, 1.1);
    cmsToneCurve* t[3] = {g, g, g};
    cmsHPROFILE in = cmsCreateLinearizationDeviceLink(cmsSigRgbData, t);
    cmsHPROFILE out = cmsCreateLinearizationDeviceLink(cmsSigRgbData, t);
    cmsFreeToneCurve(g);
    cmsHTRANSFORM h = cmsCreateTransform(in, TYPE_RGB_8, out, TYPE_RGB_8, INTENT_PERCEPTUAL, cmsFLAGS_NOCACHE);
    unsigned char ib[3], ob[3];
    for (int r=0;r<256;r+=3) for (int gg=0;gg<256;gg+=17) for (int b=0;b<256;b+=17) {
        ib[0]=r;ib[1]=gg;ib[2]=b;
        cmsDoTransform(h,ib,ob,1);
        printf("%u %u %u -> %u %u %u\n",ib[0],ib[1],ib[2],ob[0],ob[1],ob[2]);
    }
    return 0;
}
