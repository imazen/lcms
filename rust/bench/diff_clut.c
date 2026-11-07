#include "lcms2.h"
#include <stdio.h>
#include <stdlib.h>
int main(void) {
    cmsHPROFILE in = cmsOpenProfileFromFile("test5.icc","r");
    cmsHPROFILE out = cmsOpenProfileFromFile("test3.icc","r");
    cmsHTRANSFORM h = cmsCreateTransform(in, TYPE_RGB_16, out, TYPE_RGB_16, INTENT_PERCEPTUAL, cmsFLAGS_NOCACHE);
    unsigned short ibuf[3], obuf[3];
    /* dense-ish sweep hitting all tetra arms + boundaries */
    for (int r = 0; r < 65536; r += 37) {
        for (int g = 0; g < 65536; g += 4099) {
            for (int b = 0; b < 65536; b += 4099) {
                ibuf[0]=r; ibuf[1]=g; ibuf[2]=b;
                cmsDoTransform(h, ibuf, obuf, 1);
                printf("%u %u %u -> %u %u %u\n", ibuf[0],ibuf[1],ibuf[2], obuf[0],obuf[1],obuf[2]);
            }
        }
    }
    return 0;
}
