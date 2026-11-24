#include "lcms2.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#ifndef INSPECT
static int convert(cmsHPROFILE profile, const char *path)
{
    int result = 1;
    FILE *file = fopen(path, "rb");
    unsigned char *input = NULL, *output = NULL;
    cmsHPROFILE target = NULL;
    cmsHTRANSFORM transform = NULL;
    if (!file || fseek(file, 0, SEEK_END) != 0) goto cleanup;
    long size = ftell(file);
    if (size < 0 || size % 3 != 0 || (uint64_t)size / 3 > UINT32_MAX) goto cleanup;
    if (fseek(file, 0, SEEK_SET) != 0) goto cleanup;
    input = malloc(size ? (size_t)size : 1);
    output = malloc(size ? (size_t)size : 1);
    if (!input || !output || fread(input, 1, (size_t)size, file) != (size_t)size) goto cleanup;
    target = cmsCreate_sRGBProfile();
    if (!target) goto cleanup;
    transform = cmsCreateTransform(profile, TYPE_RGB_8, target, TYPE_RGB_8,
                                   INTENT_RELATIVE_COLORIMETRIC, 0);
    if (!transform) goto cleanup;
    cmsDoTransform(transform, input, output, (cmsUInt32Number)(size / 3));

    uint32_t hash = 2166136261u;
    for (long i = 0; i < size; ++i) hash = (hash ^ output[i]) * 16777619u;
    printf("%08x\n", hash);
    result = 0;

cleanup:
    if (transform) cmsDeleteTransform(transform);
    if (target) cmsCloseProfile(target);
    if (file) fclose(file);
    free(input);
    free(output);
    return result;
}
#endif

int main(int argc, char **argv)
{
#ifdef INSPECT
    if (argc != 2) return 1;
#else
    if (argc != 3) return 1;
#endif
    cmsHPROFILE profile = cmsOpenProfileFromFile(argv[1], "r");
    if (!profile) return 1;
#ifdef INSPECT
    char description[1024] = {0};
    cmsGetProfileInfoASCII(profile, cmsInfoDescription, "en", "US",
                           description, sizeof(description));
    printf("%08x\t%08x\t%08x\t%s\n", cmsGetEncodedICCversion(profile),
           cmsGetColorSpace(profile), cmsGetPCS(profile), description);
    int result = 0;
#else
    int result = convert(profile, argv[2]);
#endif
    cmsCloseProfile(profile);
    return result;
}
