// SPDX-License-Identifier: GPL-2.0-or-later
#include <cassert>
#include <cstdio>

#include "bbport_resize.h"

int main() {
    // 1. Threshold and zero-size guards.
    assert(!BbResizeNeedsRestart(0, 0, 2208, 1840));
    assert(!BbResizeNeedsRestart(2092, 1080, 0, 0));
    assert(!BbResizeNeedsRestart(1920, 1080, 1920, 1080));
    assert(!BbResizeNeedsRestart(1920, 1080, 1936, 1096)); // exactly 16 px delta
    assert(BbResizeNeedsRestart(1920, 1080, 1937, 1080));  // > 16 px delta

    // Pixel Fold cover (2092x1080) <-> unfolded (2208x1840) and desktop resizes.
    assert(BbResizeNeedsRestart(2092, 1080, 2208, 1840));
    assert(BbResizeNeedsRestart(2208, 1840, 2092, 1080));
    assert(BbResizeNeedsRestart(1280, 720, 1920, 1080));
    assert(BbResizeNeedsRestart(1920, 1080, 2560, 1440));

    // 2. Debounced tracker: Pixel Fold cover -> unfolded with intermediate animation sizes.
    BbResizeTracker tracker(250);
    assert(!tracker.Observe(2092, 1080, 2092, 1080, 0).needs_restart);
    assert(!tracker.Observe(2092, 1080, 2092, 1080, 300).needs_restart);

    assert(!tracker.Observe(2092, 1080, 2140, 1300, 1000).needs_restart);
    assert(!tracker.Observe(2092, 1080, 2180, 1600, 1100).needs_restart);
    BbResizeState s = tracker.Observe(2092, 1080, 2208, 1840, 1200);
    assert(!s.needs_restart);
    assert(!s.just_settled);

    // Still within 250 ms debounce window.
    s = tracker.Observe(2092, 1080, 2208, 1840, 1400);
    assert(!s.needs_restart);

    // Settles once stable for >= 250 ms.
    s = tracker.Observe(2092, 1080, 2208, 1840, 1450);
    assert(s.needs_restart);
    assert(s.just_settled);
    assert(s.settled_width == 2208 && s.settled_height == 1840);

    // Steady frames do not re-trigger just_settled.
    s = tracker.Observe(2092, 1080, 2208, 1840, 1600);
    assert(s.needs_restart);
    assert(!s.just_settled);

    // Minimized 0x0 frame is ignored without clearing or altering settled state.
    s = tracker.Observe(2092, 1080, 0, 0, 1700);
    assert(s.needs_restart);
    assert(s.settled_width == 2208 && s.settled_height == 1840);

    // Resizing to a different diverged desktop size re-triggers just_settled once debounced.
    assert(tracker.Observe(2092, 1080, 2560, 1440, 1800).needs_restart);
    s = tracker.Observe(2092, 1080, 2560, 1440, 2050);
    assert(s.needs_restart);
    assert(s.just_settled);
    assert(s.settled_width == 2560 && s.settled_height == 1440);

    // Folding back to cover (within 16 px of startup) clears needs_restart once debounced.
    assert(tracker.Observe(2092, 1080, 2092, 1080, 2200).needs_restart);
    s = tracker.Observe(2092, 1080, 2092, 1080, 2450);
    assert(!s.needs_restart);
    assert(!s.just_settled);
    assert(s.settled_width == 2092 && s.settled_height == 1080);

    // 3. Startup baseline is the first swapchain extent (#640): no prompt at startup when the
    // render size differs from the window (3328x1396 render in 3440x1440, 2560x1440 in 1600x900).
    BbResizeTracker startup(250);
    s = startup.ObserveExtent(0, 0, 0); // minimized before the first frame: no baseline yet
    assert(!s.needs_restart && startup.StartWidth() == 0);
    s = startup.ObserveExtent(1600, 900, 10);
    assert(startup.StartWidth() == 1600 && startup.StartHeight() == 900);
    s = startup.ObserveExtent(1600, 900, 300);
    assert(!s.needs_restart && !s.just_settled);
    s = startup.ObserveExtent(1600, 900, 5000);
    assert(!s.needs_restart && !s.just_settled);

    // A real resize still prompts once debounced, and resizing back clears it.
    assert(!startup.ObserveExtent(2560, 1440, 6000).needs_restart);
    s = startup.ObserveExtent(2560, 1440, 6250);
    assert(s.needs_restart && s.just_settled);
    assert(s.settled_width == 2560 && s.settled_height == 1440);
    assert(startup.StartWidth() == 1600 && startup.StartHeight() == 900);
    startup.ObserveExtent(1600, 900, 7000);
    s = startup.ObserveExtent(1600, 900, 7250);
    assert(!s.needs_restart && !s.just_settled);

    std::printf("runtime-resize-test: all assertions passed\n");
    return 0;
}
