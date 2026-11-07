#include "lcms2.h"
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
int main(int argc, char**argv) {
    const char *mode = argc>1?argv[1]:"16clut";
    cmsHPROFILE in, out; cmsUInt32Number fin, fout;
    if (mode[0]=='c') { in=cmsOpenProfileFromFile("test1.icc","r"); out=cmsOpenProfileFromFile("test2.icc","r"); fin=TYPE_CMYK_8; fout=TYPE_CMYK_8; }
    else { in=cmsOpenProfileFromFile("test5.icc","r"); out=cmsOpenProfileFromFile("test3.icc","r"); fin=TYPE_RGBA_16; fout=TYPE_RGBA_16; }
    if(!in||!out){fprintf(stderr,"no prof %s\n",mode);return 1;}
    cmsHTRANSFORM t = cmsCreateTransform(in, fin, out, fout, 0, 0);
    if (!t) { fprintf(stderr,"no xform\n"); return 1; }
    int n = 1<<20;
    unsigned char *ib = malloc(n*8), *ob = malloc(n*8);
    for (int i=0;i<n*8;i++) ib[i] = i*31;
    struct timespec a,b; double best=1e9;
    for (int r=0;r<10;r++) { clock_gettime(CLOCK_MONOTONIC,&a); cmsDoTransform(t,ib,ob,n); clock_gettime(CLOCK_MONOTONIC,&b);
        double d=(b.tv_sec-a.tv_sec)+1e-9*(b.tv_nsec-a.tv_nsec); if(d<best)best=d; }
    printf("%s: %.2f MPix/s\n", mode, n/best/1e6);
    return 0;
}
