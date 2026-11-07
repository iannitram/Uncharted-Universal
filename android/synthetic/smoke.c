#include <SDL3/SDL.h>

int main(void) {
    SDL_version version;
    SDL_GetVersion(&version);
    return version.major == 3 ? 0 : 1;
}
