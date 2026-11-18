/* The frame-pacing patch in probe.c: patch_eop_tick_wait finds the game's "EOP tick" assertion
 * through its message and turns the failed check into pause + jmp rel32 back to the check (f3 90 e9 rel32), and only where
 * the assertion path ends in an int 0x41 trap shortly after the message load. These
 * tests feed it small synthetic x86-64 images (no game bytes) with the instruction shapes the
 * matcher accepts, and a few that it must leave alone. probe.c is included whole, with its
 * main renamed, so the static function and the image global are reachable. */
#define _GNU_SOURCE
/* probe's main falls off its end (implicitly 0); renamed, that is a -Wreturn-type error */
#pragma GCC diagnostic ignored "-Wreturn-type"
#define main bb_probe_main
#include "../src/probe.c"
#undef main
#include <assert.h>

static const char message[]="GetRenderFrameParams(frameToFreeInstancesFor)->m_gfxEopTick";
#define SIZE 0x800u
#define MESSAGE_AT 0x600u

static unsigned char buffer[SIZE], original[SIZE];

static void reset(void) {
    memset(buffer,0x90,sizeof(buffer));
    memcpy(buffer+MESSAGE_AT,message,sizeof(message));
}

/* One assertion site at `at`: cmp (ends with the imm8 0), jne over the assertion path, 8 bytes of the
 * (dead) path, `lea reg,[rip+message]`, then the int 0x41 trap the matcher requires; the jne goes to
 * just after the trap. With trap=0 the int 0x41 is left out (the bytes stay 0xcc). Returns the offset
 * of the jne. */
static size_t site(size_t at, const unsigned char *cmp, size_t cmp_length, int lea_register, int trap) {
    memcpy(buffer+at,cmp,cmp_length);
    size_t jne=at+cmp_length;
    memset(buffer+jne+2,0xcc,8+7+2);
    size_t lea=jne+2+8;
    buffer[lea]=0x48; buffer[lea+1]=0x8d; buffer[lea+2]=(unsigned char)(0x05|lea_register<<3);
    int32_t rel=(int32_t)((int64_t)MESSAGE_AT-(int64_t)(lea+7));
    memcpy(buffer+lea+3,&rel,4);
    if (trap) { buffer[lea+7]=0xcd; buffer[lea+8]=0x41; }
    buffer[jne]=0x75; buffer[jne+1]=(unsigned char)(8+7+2);     /* to just after the trap */
    return jne;
}

static void run(void) {
    memcpy(original,buffer,sizeof(buffer));
    image=buffer;
    patch_eop_tick_wait(SIZE);
}

/* The 7 bytes after the jne (the assertion path) became pause; jmp rel32 back to the cmp, with the
 * displacement computed here from the end of the jmp; nothing else changed. */
static void expect_patched(size_t cmp_at, size_t jne) {
    const size_t path=jne+2;
    assert(buffer[path]==0xf3 && buffer[path+1]==0x90 && buffer[path+2]==0xe9);
    int32_t rel; memcpy(&rel,buffer+path+3,4);
    assert(rel==(int32_t)((int64_t)cmp_at-(int64_t)(path+7)));
    for (size_t i=0;i<SIZE;++i)
        if (i<path || i>=path+7) assert(buffer[i]==original[i]);
}

static void expect_unchanged(void) { assert(!memcmp(buffer,original,SIZE)); }

int main(void) {
    /* 1.00 shape: cmp qword [rax+0xa08], 0 (mod 10, no SIB) with lea rdi */
    static const unsigned char disp32[]={0x48,0x83,0xb8,0x08,0x0a,0,0,0x00};
    /* US 1.02 shape: cmp qword [rax+rcx+0xa08], 0 (mod 10, SIB) */
    static const unsigned char disp32_sib[]={0x48,0x83,0xbc,0x08,0x08,0x0a,0,0,0x00};
    /* disp8 forms, without and with SIB, and a REX.WB base register */
    static const unsigned char disp8[]={0x48,0x83,0x78,0x10,0x00};
    static const unsigned char disp8_sib[]={0x48,0x83,0x7c,0x08,0x10,0x00};
    static const unsigned char rex_b[]={0x49,0x83,0xbf,0x08,0x0a,0,0,0x00};
    struct { const unsigned char *cmp; size_t length; } shapes[]={
        {disp32,sizeof(disp32)},{disp32_sib,sizeof(disp32_sib)},{disp8,sizeof(disp8)},
        {disp8_sib,sizeof(disp8_sib)},{rex_b,sizeof(rex_b)}};

    /* every accepted shape, alone, with lea rdi (register 7) */
    for (size_t s=0;s<sizeof(shapes)/sizeof(*shapes);++s) {
        reset();
        size_t jne=site(0x100,shapes[s].cmp,shapes[s].length,7,1);
        run();
        expect_patched(0x100,jne);
    }
    /* any register in the lea (REX.W 8d /r with mod 00 rm 101) */
    for (int r=0;r<8;++r) {
        reset();
        size_t jne=site(0x100,disp32,sizeof(disp32),r,1);
        run();
        expect_patched(0x100,jne);
    }
    /* all of them in one image: the matcher patches each site, not only the first */
    reset();
    size_t jnes[5];
    for (size_t s=0;s<5;++s) jnes[s]=site(0x80*(s+1),shapes[s].cmp,shapes[s].length,7,1);
    run();
    for (size_t s=0;s<5;++s) {
        const size_t path=jnes[s]+2;
        assert(buffer[path]==0xf3 && buffer[path+1]==0x90 && buffer[path+2]==0xe9);
        int32_t rel; memcpy(&rel,buffer+path+3,4);
        assert(rel==(int32_t)((int64_t)(0x80*(s+1))-(int64_t)(path+7)));
    }

    /* near jne (0f 85 rel32) over the path: patched too; the path starts after the 6-byte jne */
    reset();
    {
        const size_t at=0x100, jne=at+sizeof(disp32), path=jne+6;
        memcpy(buffer+at,disp32,sizeof(disp32));
        buffer[jne]=0x0f; buffer[jne+1]=0x85;
        int32_t d=8+7+2; memcpy(buffer+jne+2,&d,4);
        memset(buffer+path,0xcc,8+7+2);
        const size_t lea=path+8;
        buffer[lea]=0x48; buffer[lea+1]=0x8d; buffer[lea+2]=0x3d;
        int32_t rel=(int32_t)((int64_t)MESSAGE_AT-(int64_t)(lea+7));
        memcpy(buffer+lea+3,&rel,4);
        buffer[lea+7]=0xcd; buffer[lea+8]=0x41;
        run();
        assert(buffer[path]==0xf3 && buffer[path+1]==0x90 && buffer[path+2]==0xe9);
        int32_t back; memcpy(&back,buffer+path+3,4);
        assert(back==(int32_t)((int64_t)at-(int64_t)(path+7)));
        for (size_t i=0;i<SIZE;++i)
            if (i<path || i>=path+7) assert(buffer[i]==original[i]);
    }

    /* negative: the assertion path has no int 0x41 trap after the message load: not the assertion
     * the matcher knows, so it must be left alone (this is the case the old shape test lacked) */
    reset();
    site(0x100,disp32,sizeof(disp32),7,0);
    run();
    expect_unchanged();

    /* negative: the trap is there but too far after the message load (the matcher looks 0x80 on) */
    reset();
    site(0x100,disp32,sizeof(disp32),7,0);
    buffer[0x100+sizeof(disp32)+2+8+7+0x80]=0xcd;
    buffer[0x100+sizeof(disp32)+2+8+7+0x80+1]=0x41;
    run();
    expect_unchanged();

    /* negative: a cmp [mem],0 and jne and a trap, but no load of the message (no lea) */
    reset();
    memcpy(buffer+0x100,disp32,sizeof(disp32));
    buffer[0x100+sizeof(disp32)]=0x75; buffer[0x100+sizeof(disp32)+1]=0x0a;
    buffer[0x100+sizeof(disp32)+2+8]=0xcd; buffer[0x100+sizeof(disp32)+2+8+1]=0x41;
    run();
    expect_unchanged();

    /* negative: a cmp [mem],0 and jne, but no assertion path (no lea, no trap) after it */
    reset();
    memcpy(buffer+0x100,disp32,sizeof(disp32));
    buffer[0x100+sizeof(disp32)]=0x75; buffer[0x100+sizeof(disp32)+1]=0x0a;
    run();
    expect_unchanged();

    /* negative: the image has no message at all */
    reset();
    site(0x100,disp32,sizeof(disp32),7,1);
    memset(buffer+MESSAGE_AT,'x',sizeof(message));
    run();
    expect_unchanged();

    /* negative: lea points at the message, with its trap, but nothing before it is a cmp [mem],0 */
    reset();
    size_t lea=0x100+10;
    buffer[lea]=0x48; buffer[lea+1]=0x8d; buffer[lea+2]=0x3d;
    int32_t rel=(int32_t)((int64_t)MESSAGE_AT-(int64_t)(lea+7));
    memcpy(buffer+lea+3,&rel,4);
    buffer[lea+7]=0xcd; buffer[lea+8]=0x41;
    run();
    expect_unchanged();

    /* negative: the jne target lies before the trap (a loop, not a skip) */
    reset();
    size_t jne=site(0x100,disp32,sizeof(disp32),7,1);
    buffer[jne+1]=0xf0;
    run();
    expect_unchanged();

    /* negative: no room for the 7 patched bytes between the jne and the end of the trap:
     * lea, then cmp + jne, then the trap right after the jne */
    reset();
    lea=0x100;
    buffer[lea]=0x48; buffer[lea+1]=0x8d; buffer[lea+2]=0x3d;
    rel=(int32_t)((int64_t)MESSAGE_AT-(int64_t)(lea+7));
    memcpy(buffer+lea+3,&rel,4);
    memcpy(buffer+lea+7,disp32,sizeof(disp32));
    jne=lea+7+sizeof(disp32);
    buffer[jne]=0x75; buffer[jne+1]=0x02;
    buffer[jne+2]=0xcd; buffer[jne+3]=0x41;
    run();
    expect_unchanged();

    /* Cases added by the merge review of fc43d47 / 35aa3f5. */
    /* lea with REX.R (4c 8d /r: r8..r15) is accepted too */
    for (int r=0;r<8;++r) {
        reset();
        size_t j=site(0x100,disp32,sizeof(disp32),r,1);
        buffer[j+2+8]=0x4c;
        run();
        expect_patched(0x100,j);
    }
    /* trap search window: lea at L, trap at L+d, near jne past the trap. d=0x7f patched, d=0x80 not. */
    for (int k=0;k<2;++k) {
        const size_t d=k ? 0x80 : 0x7f;
        reset();
        const size_t at=0x100, j=at+sizeof(disp32), path=j+6, L=path+8, trap=L+d;
        memcpy(buffer+at,disp32,sizeof(disp32));
        buffer[j]=0x0f; buffer[j+1]=0x85;
        int32_t dd=(int32_t)((int64_t)(trap+2)-(int64_t)(j+6)); memcpy(buffer+j+2,&dd,4);
        buffer[L]=0x48; buffer[L+1]=0x8d; buffer[L+2]=0x3d;
        int32_t rr=(int32_t)((int64_t)MESSAGE_AT-(int64_t)(L+7)); memcpy(buffer+L+3,&rr,4);
        buffer[trap]=0xcd; buffer[trap+1]=0x41;
        run();
        if (k) expect_unchanged();
        else {
            assert(buffer[path]==0xf3 && buffer[path+1]==0x90 && buffer[path+2]==0xe9);
            int32_t b; memcpy(&b,buffer+path+3,4);
            assert(b==(int32_t)((int64_t)at-(int64_t)(path+7)));
        }
    }
    /* je to the assertion path (short 74 rel8 and near 0f 84 rel32): the path is the je target */
    for (int nearj=0;nearj<2;++nearj) {
        reset();
        const size_t at=0x100, j=at+sizeof(disp32), after=j+(nearj ? 6 : 2), path=after+4, L=path+8;
        memcpy(buffer+at,disp32,sizeof(disp32));
        if (nearj) { buffer[j]=0x0f; buffer[j+1]=0x84; int32_t dd=4; memcpy(buffer+j+2,&dd,4); }
        else { buffer[j]=0x74; buffer[j+1]=4; }
        memset(buffer+path,0xcc,8);
        buffer[L]=0x48; buffer[L+1]=0x8d; buffer[L+2]=0x3d;
        int32_t rr=(int32_t)((int64_t)MESSAGE_AT-(int64_t)(L+7)); memcpy(buffer+L+3,&rr,4);
        buffer[L+7]=0xcd; buffer[L+8]=0x41;
        run();
        assert(buffer[path]==0xf3 && buffer[path+1]==0x90 && buffer[path+2]==0xe9);
        int32_t b; memcpy(&b,buffer+path+3,4);
        assert(b==(int32_t)((int64_t)at-(int64_t)(path+7)));
        for (size_t i=0;i<SIZE;++i) if (i<path || i>=path+7) assert(buffer[i]==original[i]);
    }
    /* negatives on the cmp: imm8 not 0, and ModRM reg field not /7 (48 83 /0 is add) */
    {
        static const unsigned char imm1[]={0x48,0x83,0x78,0x10,0x01};
        static const unsigned char add0[]={0x48,0x83,0x80,0x08,0x0a,0,0,0x00};
        reset(); site(0x100,imm1,sizeof(imm1),7,1); run(); expect_unchanged();
        reset(); site(0x100,add0,sizeof(add0),7,1); run(); expect_unchanged();
    }
    /* room, at the boundary: the 7 patched bytes may end exactly at the end of the trap, not one past */
    for (int k=0;k<2;++k) {
        reset();
        const size_t L=0x100, c=L+7, j=c+sizeof(disp32), path=j+2, trap=path+(k ? 4 : 5);
        buffer[L]=0x48; buffer[L+1]=0x8d; buffer[L+2]=0x3d;
        int32_t rr=(int32_t)((int64_t)MESSAGE_AT-(int64_t)(L+7)); memcpy(buffer+L+3,&rr,4);
        memcpy(buffer+c,disp32,sizeof(disp32));
        buffer[j]=0x75; buffer[j+1]=(unsigned char)(trap+2-path);
        memset(buffer+path,0xcc,trap-path);
        buffer[trap]=0xcd; buffer[trap+1]=0x41;
        run();
        if (k) expect_unchanged(); else expect_patched(c,j);
    }

    puts("eop matcher tests passed");
    return 0;
}
