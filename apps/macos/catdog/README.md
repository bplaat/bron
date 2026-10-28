# CatDog

A native macOS AppKit clone of [SerenityOS CatDog](https://github.com/SerenityOS/serenity/tree/master/Userland/Demos/CatDog).
The original 32x32 sprites render at 2x size (64x64 points) with sharp pixel edges, including on Retina displays.
The companion follows the pointer in eight directions, falls asleep after five seconds of idle time,
and offers advice every fifteen seconds while awake. Artist and inspector appearances react to running art apps and
Activity Monitor or Instruments.

Requires macOS 11 or newer and the Xcode command line tools. Build and launch with:

```sh
make run
```

- Click CatDog to toggle advice.
- Click a speech bubble to dismiss it and resume following the pointer.
- Right-click CatDog or use its menu bar icon to toggle sleep, wake it, or quit.

CatDog floats above windows, joins all Spaces, and stays within the usable screen area. It runs without a Dock icon.
It walks smoothly between displays, speeding up while crossing to another screen.
Pointer tracking uses AppKit's mouse location and does not require Accessibility or Input Monitoring permission.

Run `make` to build `CatDog.app`, or `make clean` to remove it.
Sprites are bundled for offline use.
SerenityOS artwork and adapted behavior are covered by [LICENSE.serenity](LICENSE.serenity), also included in the app bundle.
