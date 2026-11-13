#define _GNU_SOURCE
#include "runtime.h"
#include <stdlib.h>

/* The settings menu of the GPU library restarts through probe.c, which tests do not link. */
void runtime_restart(void) { abort(); }
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <pthread.h>
/* Every exit and STOP of the runtime ends through runtime_quit, which is in probe.c
 * (upstream 0.3.14). Tests end at once, with the output flushed. */
void runtime_quit(int code) { fflush(NULL); _Exit(code); }
#include <time.h>

typedef int32_t (ABI *AttrInit)(void **);
typedef int32_t (ABI *HandleOp)(void **);
typedef int32_t (ABI *Allocate)(int64_t, int64_t, uint64_t, uint64_t, int, int64_t *);
typedef int32_t (ABI *Map)(void **, uint64_t, int, int, int64_t, uint64_t);
typedef int32_t (ABI *Unmap)(void *, uint64_t);
typedef int32_t (ABI *Release)(uint64_t, uint64_t);
#define GET(type, name) ((type)runtime_resolve(name, 0))
typedef void *(ABI *ThreadSelf)(void);
typedef int32_t (ABI *ThreadGetAttr)(void *,void **);
typedef int32_t (ABI *ThreadAffinity)(void **,uint64_t *);
static void *main_identity;
static void *identity_worker(void *unused) {
    (void)unused;
    assert(GET(ThreadSelf,"aI+OeCz8xrQ#p#J")()!=main_identity);
    return NULL;
}
/* The libc contracts (TLS, exit handlers, guards, memory primitives, process param, heap API)
 * are crates/runtime's abi::libc now, tested there; this is the thread module's part. */
static void thread_attributes(void) {
    main_identity=GET(ThreadSelf,"aI+OeCz8xrQ#p#J")();
    pthread_t worker;
    assert(pthread_create(&worker,NULL,identity_worker,NULL)==0);
    assert(pthread_join(worker,NULL)==0);
    void *attr=NULL;
    AttrInit init=GET(AttrInit,"nsYoNRywwNg#p#J");
    ThreadGetAttr get=GET(ThreadGetAttr,"x1X76arYMxU#p#J");
    ThreadAffinity affinity=GET(ThreadAffinity,"8+s5BzZjxSg#p#J");
    HandleOp destroy=GET(HandleOp,"62KCwEMmzcM#p#J");
    uint64_t mask=0;
    assert((uint32_t)init(NULL)==0x80020016);
    assert(init(&attr)==0 && get(main_identity,&attr)==0);
    assert(affinity(&attr,&mask)==0 && mask==0x7f);
    assert((uint32_t)get(&mask,&attr)==0x80020003);
    assert((uint32_t)affinity(&attr,NULL)==0x80020016);
    assert(destroy(&attr)==0 && !attr);
    assert((uint32_t)affinity(&attr,&mask)==0x80020016);
}
static void wall_time(void) {
    typedef int (ABI *GetTime)(int64_t *,int32_t *);
    GetTime get=GET(GetTime,"n88vx3C5nW8#I#J");
    struct { int64_t value[2]; uint64_t canary; } t={{0,0},0xabcdef};
    struct { int32_t value[2]; uint32_t canary; } z={{0,0},0x123456};
    time_t before=time(NULL);
    assert(get(t.value,z.value)==0);
    time_t after=time(NULL);
    assert(t.value[0]>=before && t.value[0]<=after);
    assert(t.value[1]>=0 && t.value[1]<1000000);
    assert(t.canary==0xabcdef && z.canary==0x123456);
    assert(get(NULL,NULL)==0 && get(NULL,z.value)==0);
    assert(!runtime_resolve("n88vx3C5nW8#q#q",0));
}
/* The memory module is Rust on Linux (crates/runtime, piece 3.12; its Rust tests port this one):
 * kept as the check that runtime_resolve reaches it across the C-to-Rust link. */
static void direct_memory(void) {
    Allocate alloc=GET(Allocate,"rTXw65xmLIA#p#J");
    Map map=GET(Map,"L-Q3LEjIbgA#p#J");
    Unmap unmap=GET(Unmap,"cQke9UuBQOk#p#J");
    Release release=GET(Release,"MBuItvba6z8#p#J");
    int64_t a=-1,b=-1,c=-1;
    const uint64_t pool=536870912, length=16384;
    assert((uint32_t)alloc(0,pool,1,0,0,&a)==0x80020016 && a==-1);
    assert((uint32_t)alloc(0,8192,length,0,0,&a)==0x80020023);
    assert(alloc(0,pool,length,2097152,0,&a)==0 && a==0);
    assert(alloc(0,pool,length,2097152,0,&b)==0 && b==2097152);
    assert(alloc(0,pool,length,16384,0,&c)==0 && c==16384);
    void *x=NULL,*y=NULL,*invalid=NULL;
    assert((uint32_t)map(&invalid,length*2,3,0,b,0)==0x80020016);
    assert(map(&x,length,3,0,a,2097152)==0 && !((uintptr_t)x%2097152));
    assert(map(&y,length,3,0,a,0)==0);
    assert(*(uint64_t *)x==0);
    *(uint64_t *)x=UINT64_C(0xabcdef0123456789);
    assert(*(uint64_t *)y==UINT64_C(0xabcdef0123456789)); /* real shared backing */
    assert((uint32_t)unmap((char *)x+4096,length)==0x80020016); /* unaligned */
    assert(unmap(x,length)==0 && unmap(y,length)==0);
    assert(release(a,length)==0 && release(b,length)==0 && release(c,length)==0);
    assert(alloc(0,pool,length,0,0,&a)==0 && a==0);
    x=NULL; assert(map(&x,length,3,0,a,0)==0);
    assert(*(uint64_t *)x==0); /* released allocation does not leak previous data */
    assert(release(a,length)==0); /* release also unmaps owned mapping */
}
/* Guest FPU state (Uncharted 3 Citadel NaN in look-at, issue #230): guest threads and fibers must
 * both start with the PS4's MXCSR 0x9fc0 (flush to zero, denormals are zero) and x87 control word
 * 0x037f. The thread is created from a thread holding the host default 0x1f80, as in the game. */
typedef struct { uint32_t mxcsr; uint16_t fpucw; int flushed; } FpuSeen;
static FpuSeen thread_fpu, fiber_fpu;
static void fpu_read(FpuSeen *seen) {
    __asm__ volatile("stmxcsr %0" : "=m"(seen->mxcsr));
    __asm__ volatile("fnstcw %0" : "=m"(seen->fpucw));
    volatile float tiny=1e-40f; /* a denormal: DAZ reads it as zero */
    seen->flushed=(tiny*1.0f==0.0f);
}
static void *ABI fpu_thread_entry(void *unused) { (void)unused; fpu_read(&thread_fpu); return NULL; }
static void ABI fpu_fiber_entry(uint64_t init,uint64_t run) {
    (void)init; (void)run;
    fpu_read(&fiber_fpu);
    GET(int32_t (ABI *)(uint64_t,uint64_t *),"B0ZX2hx9DMw#libSceFiber")(0,NULL);
    abort();
}
typedef int32_t (ABI *ThreadCreate)(void **,void **,void *(ABI *)(void *),void *,const char *);
typedef int32_t (ABI *ThreadJoin)(void *,void **);
typedef int32_t (ABI *FiberInit)(void *,const char *,void (ABI *)(uint64_t,uint64_t),uint64_t,void *,uint64_t,const void *,uint32_t);
typedef int32_t (ABI *FiberRun)(void *,uint64_t,uint64_t *);
static void guest_fpu_state(void) {
    unsetenv("BB_GUEST_MXCSR");
    const uint32_t host=0x1f80;
    __asm__ volatile("ldmxcsr %0" : : "m"(host)); /* what a thread inherits from the host */
    void *thread=NULL;
    assert(GET(ThreadCreate,"6UgtwV+0zb4#p#J")(&thread,NULL,fpu_thread_entry,NULL,"fpu")==0);
    assert(GET(ThreadJoin,"onNY9Byn-W8#p#J")(thread,NULL)==0);
    fprintf(stderr,"guest thread: mxcsr=%#x fpucw=%#x denormal flushed=%d\n",thread_fpu.mxcsr,thread_fpu.fpucw,thread_fpu.flushed);
    assert(thread_fpu.mxcsr==0x9fc0 && thread_fpu.fpucw==0x037f && thread_fpu.flushed);
    static _Alignas(16) unsigned char fiber[256]; static _Alignas(16) unsigned char stack[16384];
    assert(GET(FiberInit,"hVYD7Ou2pCQ#libSceFiber")(fiber,"fpu",fpu_fiber_entry,0,stack,sizeof(stack),NULL,0x03500000u)==0);
    assert(GET(FiberRun,"a0LLrZWac0M#libSceFiber")(fiber,0,NULL)==0);
    fprintf(stderr,"fiber: mxcsr=%#x fpucw=%#x denormal flushed=%d\n",fiber_fpu.mxcsr,fiber_fpu.fpucw,fiber_fpu.flushed);
    assert(fiber_fpu.mxcsr==0x9fc0 && fiber_fpu.fpucw==0x037f && fiber_fpu.flushed);
}
int main(void) {
    runtime_start(0); assert(runtime_resolve("bzQExy189ZI#q#q",0)==0);
    runtime_start(1);
    assert(runtime_resolve("bzQExy189ZI#wrong#module",0)==0);
    assert(runtime_resolve("unknown",0)==0);
    assert(runtime_resolve("8zTFvBIAIN8#q#q",1)==0);
    assert(runtime_resolve("f7uOxY9mM1U#p#J",0)==0);
    uint64_t *canary=(uint64_t *)runtime_resolve("f7uOxY9mM1U#p#J",1);
    assert(canary && *canary && !(*canary & 255));
    thread_attributes(); wall_time(); direct_memory(); guest_fpu_state();
    puts("PASS: thread attributes, shared direct memory, resolver scope, guest thread and fiber FPU state");
    return 0;
}
