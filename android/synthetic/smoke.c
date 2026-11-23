#include <SDL3/SDL.h>

int main(void) {
    const int version = SDL_GetVersion();
    return SDL_VERSIONNUM_MAJOR(version) == 3 ? 0 : 1;
}
