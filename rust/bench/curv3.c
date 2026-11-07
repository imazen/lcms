#include "lcms2.h"
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
static cmsHPROFILE mkcurves(void) {
    cmsToneCurve* g = cmsBuildGamma(NULL, 1.1);
    cmsToneCurve* t[3] = {g, g, g};
    cmsHPROFILE h = cmsCreateLinearizationDeviceLink(cmsSigRgbData, t);
    cmsFreeToneCurve(g);
    return h;
}
int main(void) {
    cmsHPROFILE in = mkcurves(), out = mkcurves();
    cmsHTRANSFORM t = cmsCreateTransform(in, TYPE_RGB_16, out, TYPE_RGB_16, INTENT_PERCEPTUAL, cmsFLAGS_NOCACHE);
    int n = 256*256*256/16;
    unsigned short *ib = malloc(n*6), *ob = malloc(n*6);
    for (int i=0;i<n*3;i++) ib[i] = (unsigned short)(i*31);
    struct timespec a,b; double best=1e9;
    for (int r=0;r<8;r++) { clock_gettime(CLOCK_MONOTONIC,&a); cmsDoTransform(t,ib,ob,n); clock_gettime(CLOCK_MONOTONIC,&b);
        double d=(b.tv_sec-a.tv_sec)+1e-9*(b.tv_nsec-a.tv_nsec); if(d<best)best=d; }
    printf("curves16: %.2f MPix/s\n", n/best/1e6);
    return 0;
}
