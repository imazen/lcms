#include <lcms2.h>
#include <stdlib.h>
#include <stdio.h>
int main(int argc, char** argv) {
    cmsHPROFILE hIn = cmsOpenProfileFromFile(argv[1], "r");
    cmsHPROFILE hOut = cmsOpenProfileFromFile(argv[2], "r");
    cmsHTRANSFORM t = cmsCreateTransform(hIn, TYPE_RGBA_16, hOut, TYPE_RGBA_16, INTENT_PERCEPTUAL, cmsFLAGS_NOCACHE);
    int n = 1000000;
    unsigned short* in = malloc(n*8); unsigned short* out = malloc(n*8);
    for (int i=0;i<n;i++){ in[i*4]=i&0xffff; in[i*4+1]=(i*3)&0xffff; in[i*4+2]=(i*7)&0xffff; in[i*4+3]=0xffff; }
    cmsDoTransform(t, in, out, n);
    printf("%d px done\n", n);
    return 0;
}
