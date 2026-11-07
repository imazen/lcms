#include "lcms2.h"
#include <stdio.h>
#include <stdlib.h>
int main(void) {
    cmsToneCurve* g = cmsBuildGamma(NULL, 1.1);
    cmsToneCurve* t[3] = {g, g, g};
    cmsHPROFILE in = cmsCreateLinearizationDeviceLink(cmsSigRgbData, t);
    cmsHPROFILE out = cmsCreateLinearizationDeviceLink(cmsSigRgbData, t);
    cmsFreeToneCurve(g);
    cmsHTRANSFORM h = cmsCreateTransform(in, TYPE_RGB_16, out, TYPE_RGB_16, INTENT_PERCEPTUAL, cmsFLAGS_NOCACHE);
    unsigned short ibuf[3], obuf[3];
    int diffs = 0;
    for (int i = 0; i < 65536; i++) {
        ibuf[0] = ibuf[1] = ibuf[2] = (unsigned short)i;
        cmsDoTransform(h, ibuf, obuf, 1);
        printf("%u -> %u %u %u\n", (unsigned)ibuf[0], (unsigned)obuf[0], (unsigned)obuf[1], (unsigned)obuf[2]);
        (void)diffs;
    }
    cmsDeleteTransform(h);
    return 0;
}
