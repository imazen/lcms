#include "lcms2.h"
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
int main(void) {
    cmsHPROFILE in=cmsOpenProfileFromFile("test5.icc","r"), out=cmsOpenProfileFromFile("test3.icc","r");
    cmsHTRANSFORM t = cmsCreateTransform(in, TYPE_RGBA_FLT, out, TYPE_RGBA_FLT, 0, cmsFLAGS_NOCACHE);
    if (!t) { fprintf(stderr,"no xform\n"); return 1; }
    int n = 1<<20;
    float *ib = malloc(n*4*4), *ob = malloc(n*4*4);
    for (int i=0;i<n*4;i++) ib[i] = (i*7 & 0xff)/255.0f;
    struct timespec a,b; double best=1e9;
    for (int r=0;r<10;r++) { clock_gettime(CLOCK_MONOTONIC,&a); cmsDoTransform(t,ib,ob,n); clock_gettime(CLOCK_MONOTONIC,&b);
        double d=(b.tv_sec-a.tv_sec)+1e-9*(b.tv_nsec-a.tv_nsec); if(d<best)best=d; }
    printf("32clut: %.2f MPix/s\n", n/best/1e6);
    return 0;
}
