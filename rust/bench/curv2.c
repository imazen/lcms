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
    cmsHTRANSFORM t = cmsCreateTransform(in, TYPE_RGBA_16, out, TYPE_RGBA_16, 0, 0);
    int n = 1<<20;
    unsigned char *ib = malloc(n*8), *ob = malloc(n*8);
    for (int i=0;i<n*8;i++) ib[i] = i*31;
    struct timespec a,b; double best=1e9;
    for (int r=0;r<10;r++) { clock_gettime(CLOCK_MONOTONIC,&a); cmsDoTransform(t,ib,ob,n); clock_gettime(CLOCK_MONOTONIC,&b);
        double d=(b.tv_sec-a.tv_sec)+1e-9*(b.tv_nsec-a.tv_nsec); if(d<best)best=d; }
    printf("curves16: %.2f MPix/s\n", n/best/1e6);
    return 0;
}
