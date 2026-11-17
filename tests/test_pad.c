#define _GNU_SOURCE
#include <assert.h>
#include <unistd.h>
#include "../src/runtime_pad.c"

_Thread_local int runtime_ndgi_open; /* runtime_fiber.c */
static int capture;
static int rumble_calls;
static uint16_t rumble_low, rumble_high;
int bbgpu_overlay_captures_input(void) { return capture; }
void bbgpu_input_sample(void) {} /* low latency mode (upstream 0.3.14): nothing to wait for */
void bbgpu_mouse_configure(int look) { (void)look; }
int bbgpu_mouse_sample(float *dx, float *dy, uint32_t *buttons, int *wheel) {
    *dx=*dy=0; *buttons=0; *wheel=0;
    return 0;
}
int bbgpu_camera_yaw(float *yaw_degrees, double *age_seconds) { (void)yaw_degrees; (void)age_seconds; return 0; }
void bbgpu_prompts_controller(int device) { (void)device; }
static int benchmark_owns_pad; /* BB_BENCHMARK (runtime_benchmark.c): the scripted pad */
int runtime_benchmark_pad(RuntimeBenchmarkPad *out) {
    if (!benchmark_owns_pad) return 0;
    out->buttons=BTN_CROSS;
    out->left_x=10; out->left_y=20; out->right_x=30; out->right_y=40;
    return 1;
}
uintptr_t runtime_lookup(const RuntimeExport *table, size_t count, const char *name) {
    (void)table; (void)count; (void)name;
    return 0;
}

static bool SDLCALL rumble(void *userdata, Uint16 low, Uint16 high) {
    (void)userdata;
    ++rumble_calls;
    rumble_low=low;
    rumble_high=high;
    return true;
}

static void set_env(const char *name, const char *value) {
#ifdef _WIN32
    assert(_putenv_s(name,value)==0);
#else
    assert(setenv(name,value,1)==0);
#endif
}

static void inject(const char *path, const char *tokens) {
    FILE *f=fopen(path,"w");
    assert(f);
    fputs(tokens,f);
    fclose(f);
    usleep(25000);
}

static void test_aim_controller_handoff(void) {
    uint64_t frame[13]={0};
    const RuntimeGuestRegs *regs=(const RuntimeGuestRegs *)frame;
    const uint64_t image=0x10000;
    frame[12]=image+0xc4c57;
    float a[3]={0,0,-1}, b[3]={0,0,-1};
    input.mouse_raw=1;
    input.mouse_sensitivity=input.mouse_aim_sensitivity=1.0f;
    aim_held=1; aim_press_us=100;
    frame[9]=(uint64_t)(uintptr_t)a;
    aim_dx=10; aim_dy=0;
    raw_aim("U1",0x1000,regs,image,0xc4c57);
    frame[9]=(uint64_t)(uintptr_t)b;
    raw_aim("U1",0x2000,regs,image,0xc4c57);
    assert(SDL_fabsf(heading_of(a)+1.5f)<0.01f);
    assert(SDL_fabsf(heading_of(b)+1.5f)<0.01f);
    /* Repeated calls get no duplicate turn; later motion reaches both controllers once. */
    raw_aim("U1",0x2000,regs,image,0xc4c57);
    assert(SDL_fabsf(heading_of(b)+1.5f)<0.01f);
    aim_dx=10;
    raw_aim("U1",0x2000,regs,image,0xc4c57);
    frame[9]=(uint64_t)(uintptr_t)a;
    raw_aim("U1",0x1000,regs,image,0xc4c57);
    assert(SDL_fabsf(heading_of(a)+3.0f)<0.01f);
    assert(SDL_fabsf(heading_of(b)+3.0f)<0.01f);
}

static void test_aim_next_press(void) {
    uint64_t frame[13]={0};
    const RuntimeGuestRegs *regs=(const RuntimeGuestRegs *)frame;
    const uint64_t image=0x10000;
    frame[12]=image+0xc4c57;
    float a[3]={0,0,-1}, b[3]={0,0,-1};
    frame[9]=(uint64_t)(uintptr_t)a;
    aim_held=0;
    raw_aim("U1",0x1000,regs,image,0xc4c57);
    aim_held=1; ++aim_press_us;
    raw_aim("U1",0x1000,regs,image,0xc4c57);
    frame[9]=(uint64_t)(uintptr_t)b;
    raw_aim("U1",0x3000,regs,image,0xc4c57);
    assert(SDL_fabsf(heading_of(a))<0.01f && SDL_fabsf(heading_of(b))<0.01f);
    aim_dx=4;
    raw_aim("U1",0x3000,regs,image,0xc4c57);
    frame[9]=(uint64_t)(uintptr_t)a;
    raw_aim("U1",0x1000,regs,image,0xc4c57);
    assert(SDL_fabsf(heading_of(a)+0.6f)<0.01f);
    assert(SDL_fabsf(heading_of(b)+0.6f)<0.01f);
    aim_held=0;
}

static void test_frame_owner_handoff(void) {
    float dx,dy;
    camera_dx[0]=10; camera_dy[0]=0;
    frame_motion(0x1000,1000000,&dx,&dy);
    assert(dx==10 && dy==0 && camera_dx[0]==0);
    camera_dx[0]=20;
    frame_motion(0x2000,1016667,&dx,&dy);
    assert(dx==20 && dy==0 && camera_dx[0]==0);
    camera_dx[0]+=30;
    frame_motion(0x2000,1033334,&dx,&dy);
    assert(dx==30 && dy==0 && camera_dx[0]==0);
    frame_motion(0x2000,1033335,&dx,&dy);
    assert(dx==0 && dy==0);
    /* The other half of a blend still receives each turn once when it returns. */
    frame_motion(0x1000,1033336,&dx,&dy);
    assert(dx==50 && dy==0);
    frame_motion(0x1000,1033337,&dx,&dy);
    assert(dx==0 && dy==0);
    ++camera_sample; /* next frame's input sample has no motion */
    frame_motion(0x4000,1050001,&dx,&dy);
    assert(dx==0 && dy==0 && camera_dx[0]==0); /* no stale share for a new owner */
}

static void test_frame_leave_and_return(void) {
    float dx,dy;
    uint64_t now=2000000;
    ++camera_sample;
    camera_dx[0]=5; camera_dy[0]=-2;
    frame_motion(0x1000,now,&dx,&dy);
    assert(SDL_fabsf(dx-5)<0.01f && SDL_fabsf(dy+2)<0.01f);
    for (int i=0;i<120;++i) {
        ++camera_sample; now+=16667;
        camera_dx[0]=5; camera_dy[0]=-2;
        frame_motion(0x3000,now,&dx,&dy);
        assert(SDL_fabsf(dx-5)<0.01f && SDL_fabsf(dy+2)<0.01f);
        assert(camera_dx[0]==0 && camera_dy[0]==0);
    }
    ++camera_sample; now+=16667;
    camera_dx[0]=5; camera_dy[0]=-2;
    frame_motion(0x1000,now,&dx,&dy);
    printf("FRAME_RETURN: A received %.2f px, expected 5.00 px\n",dx);
    fflush(stdout);
    assert(SDL_fabsf(dx-5)<0.01f && SDL_fabsf(dy+2)<0.01f);
    /* Both cameras share this frame once, including when their call order reverses. */
    frame_motion(0x3000,now+1,&dx,&dy);
    assert(SDL_fabsf(dx-5)<0.01f && SDL_fabsf(dy+2)<0.01f);
    frame_motion(0x1000,now+2,&dx,&dy);
    assert(dx==0 && dy==0);
    frame_motion(0x3000,now+3,&dx,&dy);
    assert(dx==0 && dy==0);
    ++camera_sample; now+=16667;
    camera_dx[0]=5; camera_dy[0]=-2;
    frame_motion(0x3000,now,&dx,&dy);
    assert(SDL_fabsf(dx-5)<0.01f && SDL_fabsf(dy+2)<0.01f);
    frame_motion(0x1000,now+1,&dx,&dy);
    assert(SDL_fabsf(dx-5)<0.01f && SDL_fabsf(dy+2)<0.01f);
    frame_motion(0x3000,now+2,&dx,&dy);
    assert(dx==0 && dy==0);
    frame_motion(0x1000,now+3,&dx,&dy);
    assert(dx==0 && dy==0);
}

static void test_frame_split_reads(void) {
    for (int extra=0;extra<=4;extra+=4) {
        float dx,dy, ax=0,ay=0,bx=0,by=0;
        uint64_t now=5000000+(uint64_t)extra*100000;
        /* Both cameras are live before the split read, with no motion carried in. */
        ++camera_sample;
        camera_dx[0]=camera_dy[0]=0;
        frame_motion(0x1000,now-16667,&dx,&dy);
        frame_motion(0x2000,now-16666,&dx,&dy);
        for (int frame=0;frame<2;++frame) {
            ++camera_sample;
            camera_dx[0]=frame==0 ? 10.0f : 0.0f;
            camera_dy[0]=frame==0 ? -5.0f : 0.0f;
            frame_motion(0x1000,now,&dx,&dy);
            ax+=dx; ay+=dy;
            assert(camera_dx[0]==0 && camera_dy[0]==0);
            if (frame==0) {
                ++camera_sample; /* extra pad read between A and B */
                camera_dx[0]=(float)extra; camera_dy[0]=-(float)extra/2;
            }
            frame_motion(0x2000,now+1,&dx,&dy);
            bx+=dx; by+=dy;
            assert(camera_dx[0]==0 && camera_dy[0]==0);
            now+=16667;
        }
        printf("FRAME_SPLIT (%d px extra): A %.2f / B %.2f px, expected %.2f each\n",
               extra,ax,bx,10.0f+(float)extra);
        fflush(stdout);
        assert(SDL_fabsf(ax-(10.0f+(float)extra))<0.01f);
        assert(SDL_fabsf(bx-ax)<0.01f && SDL_fabsf(by-ay)<0.01f);
        assert(SDL_fabsf(ay-(-5.0f-(float)extra/2))<0.01f);
    }
}

static void test_frame_next_press(void) {
    float dx,dy;
    camera_dx[0]=8;
    frame_motion(0x4000,1050002,&dx,&dy);
    assert(dx==8 && dy==0);
    aim_held=1; ++aim_press_us;
    frame_motion(0x3000,1050003,&dx,&dy);
    assert(dx==0 && dy==0 && camera_dx[0]==0);
    camera_dx[0]=4; camera_dy[0]=-2;
    frame_motion(0x3000,1066670,&dx,&dy);
    assert(dx==4 && dy==-2 && camera_dx[0]==0 && camera_dy[0]==0);
    frame_motion(0x2000,1066671,&dx,&dy);
    assert(dx==4 && dy==-2); /* a camera joining this blend */
    frame_motion(0x2000,1066672,&dx,&dy);
    assert(dx==0 && dy==0);
    aim_held=0;
}

int main(void) {
    char path[]="bbport-pad-test-XXXXXX";
    int fd=mkstemp(path);
    assert(fd>=0);
    close(fd);
    char config[]="bbport-input-test-XXXXXX";
    fd=mkstemp(config);
    assert(fd>=0);
    close(fd);
    inject(config,"pad_vibration=0\npad_vibration_strength=50\n");
    set_env("BB_PAD_FILE",path);
    set_env("BB_PAD_HOST","1");
    set_env("BB_INPUT_CONFIG",config);
    set_env("SDL_VIDEODRIVER","dummy");
    /* Only the virtual test controller is a gamepad, whatever is plugged in. */
    SDL_SetHint(SDL_HINT_GAMECONTROLLER_IGNORE_DEVICES_EXCEPT,"0x1d50/0x6189");
    assert(SDL_Init(SDL_INIT_VIDEO|SDL_INIT_GAMEPAD));
    assert(pad_init()==0 && pad_open(1,0,0,NULL)==1);
    PadData data;
    inject(path,"cross l3 touchpad_left");
    assert(pad_read_state(1,&data)==0);
    assert(input.pad_vibration==0);
    assert(input.pad_vibration_strength==50);
    assert((data.buttons & (BTN_CROSS|BTN_L3|BTN_TOUCHPAD))==(BTN_CROSS|BTN_L3|BTN_TOUCHPAD));
    assert(data.touch_count==1 && data.touches[0].x==480 && data.touches[0].y==471);
    inject(path,"touchpad_right");
    assert(pad_read_state(1,&data)==0 && data.touch_count==1 && data.touches[0].x==1440);
    inject(path,"");
    assert(pad_read_state(1,&data)==0 && data.buttons==0 && data.touch_count==0);

    SDL_VirtualJoystickTouchpadDesc touch={.nfingers=2};
    SDL_VirtualJoystickDesc desc;
    SDL_INIT_INTERFACE(&desc);
    desc.type=SDL_JOYSTICK_TYPE_GAMEPAD;
    desc.naxes=SDL_GAMEPAD_AXIS_COUNT;
    desc.nbuttons=SDL_GAMEPAD_BUTTON_COUNT;
    desc.button_mask=(1u<<SDL_GAMEPAD_BUTTON_COUNT)-1;
    desc.axis_mask=(1u<<SDL_GAMEPAD_AXIS_COUNT)-1;
    desc.name="bbport test controller";
    desc.vendor_id=0x1d50;
    desc.product_id=0x6189;
    desc.ntouchpads=1;
    desc.touchpads=&touch;
    desc.Rumble=rumble;
    SDL_JoystickID id=SDL_AttachVirtualJoystick(&desc);
    assert(id!=0);
    SDL_Joystick *joystick=SDL_OpenJoystick(id);
    assert(joystick);
    assert(SDL_SetJoystickVirtualTouchpad(joystick,0,0,true,0.75f,0.5f,1.0f));
    assert(SDL_SetJoystickVirtualTouchpad(joystick,0,1,true,0.25f,1.0f,1.0f));
    assert(SDL_SetJoystickVirtualButton(joystick,SDL_GAMEPAD_BUTTON_TOUCHPAD,true));
    SDL_UpdateJoysticks();
    SDL_UpdateGamepads();
    assert(pad_read_state(1,&data)==0);
    assert(gamepad && data.touch_count==2 && (data.buttons & BTN_TOUCHPAD));
    const uint8_t vibration[]={64,192};
    assert(pad_vibration(1,vibration)==0 && rumble_calls==0);
    input.pad_vibration=1;
    assert(pad_vibration(1,vibration)==0 && rumble_calls==1);
    assert(rumble_low==(64*257+1)/2 && rumble_high==(192*257+1)/2);
    input.pad_vibration_strength=100;
    assert(pad_vibration(1,vibration)==0 && rumble_calls==2);
    assert(rumble_low==64*257 && rumble_high==192*257);
    assert(data.touches[0].x==1439 && data.touches[0].y==471 && data.touches[0].id==0);
    assert(data.touches[1].x==480 && data.touches[1].y==942 && data.touches[1].id==1);
    capture=1;
    assert(pad_read_state(1,&data)==0 && data.touch_count==0 && data.buttons==0);
    capture=0;
    /* Benchmark mode replaces the whole read: the real pad and the replay file are ignored. */
    benchmark_owns_pad=1;
    assert(pad_read_state(1,&data)==0);
    assert(data.buttons==BTN_CROSS && data.l2==0 && data.r2==0);
    assert(data.left_x==10 && data.left_y==20 && data.right_x==30 && data.right_y==40);
    benchmark_owns_pad=0;
    assert(pad_read_state(1,&data)==0 && data.buttons!=BTN_CROSS); /* the real pad again */
    assert(SDL_SetJoystickVirtualTouchpad(joystick,0,0,false,0,0,0));
    assert(SDL_SetJoystickVirtualTouchpad(joystick,0,1,false,0,0,0));
    SDL_UpdateJoysticks();
    SDL_UpdateGamepads();
    assert(pad_read_state(1,&data)==0 && data.touch_count==1 && data.touches[0].x==480);
    /* Linux fork: a second gamepad; pressing a button on it makes it the one the game reads. */
    assert(SDL_SetJoystickVirtualButton(joystick,SDL_GAMEPAD_BUTTON_TOUCHPAD,false));
    assert(SDL_SetJoystickVirtualTouchpad(joystick,0,0,false,0,0,0));
    SDL_VirtualJoystickDesc second=desc;
    second.name="bbport second test controller";
    second.ntouchpads=0;
    second.touchpads=NULL;
    SDL_JoystickID second_id=SDL_AttachVirtualJoystick(&second);
    assert(second_id!=0);
    SDL_Joystick *second_joystick=SDL_OpenJoystick(second_id);
    assert(second_joystick);
    SDL_UpdateJoysticks();
    SDL_UpdateGamepads();
    gamepads_listed_us=0;
    assert(pad_read_state(1,&data)==0 && !(data.buttons & BTN_CROSS) && gamepad_count==2);
    assert(SDL_SetJoystickVirtualButton(second_joystick,SDL_GAMEPAD_BUTTON_SOUTH,true));
    SDL_UpdateJoysticks();
    SDL_UpdateGamepads();
    assert(pad_read_state(1,&data)==0 && (data.buttons & BTN_CROSS));
    assert(gamepad && SDL_GetGamepadID(gamepad)==second_id);
    /* Linux fork: keys, clicks and wheel steps that a desktop layout makes from the controller
     * (issues #66, #75): a press next to a gamepad change is ignored, one on its own counts after
     * BB_KEY_MIRROR_MS (60). The stick counts as a gamepad change. */
    assert(SDL_SetJoystickVirtualButton(second_joystick,SDL_GAMEPAD_BUTTON_SOUTH,false));
    SDL_UpdateJoysticks();
    SDL_UpdateGamepads();
    static bool keys[SDL_SCANCODE_COUNT];
    const uint64_t t=1000000000000ull;
    uint32_t mouse=0;
    int wheel=0;
    const bool *k=keys;
    filter_mirrored_input(gamepad,t,&k,&mouse,&wheel);
    assert(SDL_SetJoystickVirtualAxis(second_joystick,SDL_GAMEPAD_AXIS_LEFTY,30000)); /* stick back */
    SDL_UpdateJoysticks();
    SDL_UpdateGamepads();
    k=keys; filter_mirrored_input(gamepad,t+1000000,&k,&mouse,&wheel);
    keys[SDL_SCANCODE_DOWN]=true; /* the layout's arrow key, 5 ms later */
    k=keys; filter_mirrored_input(gamepad,t+1005000,&k,&mouse,&wheel);
    assert(!k[SDL_SCANCODE_DOWN]);
    k=keys; filter_mirrored_input(gamepad,t+1200000,&k,&mouse,&wheel);
    assert(!k[SDL_SCANCODE_DOWN]); /* held on: still ignored */
    keys[SDL_SCANCODE_DOWN]=false;
    wheel=1; /* the trackpad's wheel step, then the stick returns 10 ms later */
    k=keys; filter_mirrored_input(gamepad,t+1300000,&k,&mouse,&wheel);
    assert(wheel==0);
    assert(SDL_SetJoystickVirtualAxis(second_joystick,SDL_GAMEPAD_AXIS_LEFTY,0));
    SDL_UpdateJoysticks();
    SDL_UpdateGamepads();
    for (uint64_t at=t+1310000; at<=t+1500000; at+=10000) {
        wheel=0; k=keys; filter_mirrored_input(gamepad,at,&k,&mouse,&wheel);
        assert(wheel==0);
    }
    keys[SDL_SCANCODE_W]=true; /* the real keyboard, the controller untouched */
    k=keys; filter_mirrored_input(gamepad,t+3000000,&k,&mouse,&wheel);
    assert(!k[SDL_SCANCODE_W]);
    k=keys; filter_mirrored_input(gamepad,t+3060000,&k,&mouse,&wheel);
    assert(k[SDL_SCANCODE_W]);
    keys[SDL_SCANCODE_W]=false;
    wheel=-1; /* the real wheel */
    k=keys; filter_mirrored_input(gamepad,t+4000000,&k,&mouse,&wheel);
    assert(wheel==0);
    k=keys; filter_mirrored_input(gamepad,t+4060000,&k,&mouse,&wheel);
    assert(wheel==-1);
    wheel=0; k=keys; filter_mirrored_input(gamepad,t+4070000,&k,&mouse,&wheel);
    assert(wheel==0); /* one step, once */
    /* Raw mouse (#393). */
    {
        test_aim_controller_handoff();
        test_aim_next_press();
        test_frame_owner_handoff();
        test_frame_next_press();
        test_frame_leave_and_return();
        test_frame_split_reads();
        input.mouse_sensitivity=1.0f; input.mouse_aim_sensitivity=1.0f;
        aim_held=0;
        const size_t camera_size=0xa000;
        float *camera=calloc(1,camera_size);
        assert(camera);
        float *dir=(float *)((char *)camera+0x9ec0);
        const uint64_t id=(uint64_t)(uintptr_t)camera;
        const RuntimeGuestRegs *no_regs=NULL;
        int states_for_camera;
        /* mouse_raw=0: the old single state (slot 0), the turn is the plain mouse turn */
        memset(follow_states,0,sizeof(follow_states));
        input.mouse_raw=0; runtime_raw_mouse_hooks=1;
        dir[0]=0.0f; dir[1]=0.0f; dir[2]=-1.0f;
        const float h0=heading_of(dir);
        camera_dx[0]=100.0f;
        u2_follow_camera(id,0,0,0,no_regs);
        assert(SDL_fabsf(wrap_degrees(h0-heading_of(dir))-15.0f)<0.05f);
        for (int i=0;i<4;++i) assert(follow_states[i].camera==0 && follow_states[i].shown_us==0);
        /* the build 100 wrapper never takes the raw path, even with mouse_raw=1 */
        input.mouse_raw=1;
        u2_follow_camera_100(id,0,0,0,no_regs);
        for (int i=0;i<4;++i) assert(follow_states[i].camera==0);
        /* raw path off where no raw hook was installed (U1 1.00 matches the same follow pattern) */
        runtime_raw_mouse_hooks=0;
        u2_follow_camera(id,0,0,0,no_regs);
        for (int i=0;i<4;++i) assert(follow_states[i].camera==0);
        /* the build 102 entry with mouse_raw=1 keeps a state of its own */
        runtime_raw_mouse_hooks=1;
        u2_follow_camera(id,0,0,0,no_regs);
        states_for_camera=0;
        for (int i=0;i<4;++i) states_for_camera+=follow_states[i].camera==id;
        assert(states_for_camera==1);
        free(camera);
        /* follow_state slot reuse: the least recently used slot goes first */
        memset(follow_states,0,sizeof(follow_states));
        FollowState *first=follow_state(10,100), *second=follow_state(11,200);
        follow_state(12,300); follow_state(13,400);
        assert(first!=second && follow_state(10,500)==first);
        FollowState *fifth=follow_state(14,600);
        assert(fifth==second && fifth->camera==14);
        assert(follow_state(10,700)==first);
        /* raw aim: a wrong return address leaves the direction alone, the right one turns it */
        uint64_t frame[13]={0};
        const RuntimeGuestRegs *regs=(const RuntimeGuestRegs *)frame;
        float aim[3]={0.0f,0.0f,-1.0f};
        frame[9]=(uint64_t)(uintptr_t)aim; /* rcx */
        const uint64_t image=0x10000;
        input.mouse_raw=1;
        aim_held=1; aim_press_us=now_us();
        frame[12]=image+0x1234;
        aim_dx=100.0f; aim_dy=0.0f;
        raw_aim("U1",0,regs,image,0xc4c57);
        assert(aim[0]==0.0f && aim[1]==0.0f && aim[2]==-1.0f && aim_dx==100.0f);
        frame[12]=image+0xc4c57;
        raw_aim("U1",0,regs,image,0xc4c57);
        assert(SDL_fabsf(wrap_degrees(heading_of(aim)+15.0f))<0.05f && aim_dx==0.0f);
        /* off: no turn */
        input.mouse_raw=0;
        aim_dx=100.0f;
        const float before=aim[0];
        raw_aim("U1",0,regs,image,0xc4c57);
        assert(aim[0]==before);
        input.mouse_raw=1;
        /* armed only when the hook ran in this press, and within the last second */
        aim_held=1; aim_press_us=5000000;
        aim_direct_us=4900000;
        assert(!raw_aim_armed(5500000));  /* the hook ran in the press before */
        aim_direct_us=5000010;
        assert(raw_aim_armed(5500000));
        assert(!raw_aim_armed(6100000));  /* a second since */
        aim_held=0;
        assert(!raw_aim_armed(5500000));
        /* injected mouse reaches the aim motion while raw aim is armed */
        aim_dx=aim_dy=0.0f;
        inject(path,"l2");
        assert(pad_read_state(1,&data)==0 && aim_held);
        aim_direct_us=now_us();
        inject(path,"l2 mx=1000");
        assert(pad_read_state(1,&data)==0);
        assert(aim_dx>1.0f);
        inject(path,"");
        assert(pad_read_state(1,&data)==0 && !aim_held);
        input.mouse_raw=0;
    }
    SDL_CloseJoystick(second_joystick);
    SDL_CloseJoystick(joystick);
    for (int i=0;i<gamepad_count;++i) SDL_CloseGamepad(gamepads[i]);
    gamepad_count=0;
    gamepad=NULL;
    assert(SDL_DetachVirtualJoystick(second_id));
    assert(SDL_DetachVirtualJoystick(id));
    SDL_Quit();
    unlink(path);
    unlink(config);
    puts("PASS: pad ABI, debug camera chord, left/right clicks, SDL touch, vibration controls, overlay capture, second gamepad, mirrored keys and wheel, raw mouse");
    return 0;
}
