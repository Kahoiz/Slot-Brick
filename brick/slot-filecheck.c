/* GPL-3.0-or-later. Portable POSIX cksum and byte comparison helper. */
#include <stdio.h>
#include <stdint.h>
#include <string.h>
#include <inttypes.h>
static uint32_t step(uint32_t crc, unsigned char b) {
    crc ^= (uint32_t)b << 24;
    for (int i=0; i<8; ++i) crc=(crc & UINT32_C(0x80000000)) ? (crc<<1)^UINT32_C(0x04c11db7) : crc<<1;
    return crc;
}
static int checksum(const char *path) {
    FILE *f=fopen(path,"rb");
    if (!f) { perror(path); return 2; }
    uint32_t crc=0; uint64_t len=0; unsigned char b[8192]; size_t n;
    while ((n=fread(b,1,sizeof b,f)) != 0) {
        for (size_t i=0;i<n;++i) crc=step(crc,b[i]);
        len += n;
    }
    int failed=ferror(f);
    if (fclose(f)) failed=1;
    if (failed) { fprintf(stderr,"Read failed: %s\n",path); return 2; }
    for (uint64_t v=len; v; v>>=8) crc=step(crc,(unsigned char)v);
    printf("%" PRIu32 " %" PRIu64 "\n",~crc,len);
    return 0;
}
static int compare(const char *a, const char *b) {
    FILE *fa=fopen(a,"rb"), *fb=fopen(b,"rb");
    if (!fa || !fb) { if(fa) fclose(fa); if(fb) fclose(fb); return 2; }
    unsigned char x[8192], y[8192]; int result=0;
    for (;;) {
        size_t nx=fread(x,1,sizeof x,fa), ny=fread(y,1,sizeof y,fb);
        if (ferror(fa)||ferror(fb)) { result=2; break; }
        if (nx!=ny || memcmp(x,y,nx)) { result=1; break; }
        if (!nx) break;
    }
    if (fclose(fa)) result=2;
    if (fclose(fb)) result=2;
    return result;
}
int main(int argc,char **argv) {
    if (argc==2 && !strcmp(argv[1],"--version")) { puts("slot-filecheck 1"); return 0; }
    if (argc==3 && !strcmp(argv[1],"checksum")) return checksum(argv[2]);
    if (argc==4 && !strcmp(argv[1],"compare")) return compare(argv[2],argv[3]);
    fputs("Usage: slot-filecheck checksum FILE | compare FILE FILE\n",stderr); return 2;
}
