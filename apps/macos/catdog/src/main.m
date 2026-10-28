/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 *
 * CatDog behavior and messages adapted from SerenityOS; see LICENSE.serenity.
 */

#import <Cocoa/Cocoa.h>
#include <math.h>

static const CGFloat PetSize = 64;

static CGFloat Clamp(CGFloat value, CGFloat minimum, CGFloat maximum) {
    return MIN(MAX(value, minimum), maximum);
}

static NSScreen* ScreenAtPoint(NSPoint point) {
    for (NSScreen* screen in NSScreen.screens) {
        if (NSPointInRect(point, screen.frame))
            return screen;
    }
    return NSScreen.mainScreen;
}

static NSPanel* CreatePanel(NSRect frame) {
    NSPanel* panel =
        [[NSPanel alloc] initWithContentRect:frame
                                   styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                                     backing:NSBackingStoreBuffered
                                       defer:NO];
    panel.opaque = NO;
    panel.backgroundColor = NSColor.clearColor;
    panel.hasShadow = NO;
    panel.level = NSFloatingWindowLevel;
    panel.hidesOnDeactivate = NO;
    panel.collectionBehavior =
        NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorFullScreenAuxiliary;
    panel.releasedWhenClosed = NO;
    return panel;
}

@interface CatDogView : NSView
@property(nonatomic, strong) NSImage* image;
@property(nonatomic, copy) void (^onClick)(void);
@end

@implementation CatDogView

- (void)drawRect:(NSRect)dirtyRect {
    [NSGraphicsContext currentContext].imageInterpolation = NSImageInterpolationNone;
    [self.image drawInRect:self.bounds];
}

- (void)mouseDown:(NSEvent*)event {
    self.onClick();
}

@end

@interface SpeechBubble : NSView
@property(nonatomic, copy) NSString* message;
@property(nonatomic) CGFloat tailX;
@property(nonatomic) BOOL belowPet;
@property(nonatomic, copy) void (^onDismiss)(void);
@end

@implementation SpeechBubble

- (void)drawRect:(NSRect)dirtyRect {
    CGFloat bottom = self.belowPet ? 1 : 11;
    CGFloat top = self.belowPet ? NSHeight(self.bounds) - 11 : NSHeight(self.bounds) - 1;
    CGFloat x = self.tailX;
    NSBezierPath* path = [NSBezierPath bezierPath];
    [path moveToPoint:NSMakePoint(1, bottom)];
    if (!self.belowPet) {
        [path lineToPoint:NSMakePoint(x - 5, bottom)];
        [path lineToPoint:NSMakePoint(x, 1)];
        [path lineToPoint:NSMakePoint(x + 5, bottom)];
    }
    [path lineToPoint:NSMakePoint(NSWidth(self.bounds) - 1, bottom)];
    [path lineToPoint:NSMakePoint(NSWidth(self.bounds) - 1, top)];
    if (self.belowPet) {
        [path lineToPoint:NSMakePoint(x + 5, top)];
        [path lineToPoint:NSMakePoint(x, NSHeight(self.bounds) - 1)];
        [path lineToPoint:NSMakePoint(x - 5, top)];
    }
    [path lineToPoint:NSMakePoint(1, top)];
    [path closePath];
    [[NSColor colorWithSRGBRed:234 / 255.0 green:246 / 255.0 blue:136 / 255.0 alpha:1] setFill];
    [path fill];
    [NSColor.blackColor setStroke];
    [path stroke];

    NSMutableParagraphStyle* paragraph = [NSMutableParagraphStyle new];
    paragraph.alignment = NSTextAlignmentCenter;
    NSDictionary* attributes = @{
        NSFontAttributeName : [NSFont systemFontOfSize:12],
        NSForegroundColorAttributeName : NSColor.blackColor,
        NSParagraphStyleAttributeName : paragraph,
    };
    NSSize size = [self.message sizeWithAttributes:attributes];
    [self.message
            drawInRect:NSMakeRect(8, bottom + (top - bottom - size.height) / 2, NSWidth(self.bounds) - 16, size.height)
        withAttributes:attributes];
}

- (void)mouseDown:(NSEvent*)event {
    self.onDismiss();
}

@end

@interface AppDelegate : NSObject <NSApplicationDelegate, NSMenuDelegate>
@property(nonatomic, strong) NSPanel* petWindow;
@property(nonatomic, strong) NSPanel* adviceWindow;
@property(nonatomic, strong) CatDogView* petView;
@property(nonatomic, strong) SpeechBubble* bubble;
@property(nonatomic, strong) NSStatusItem* statusItem;
@property(nonatomic, strong) NSMenuItem* sleepItem;
@property(nonatomic, strong) NSMenuItem* adviceItem;
@property(nonatomic, strong) NSDictionary<NSString*, NSImage*>* images;
@property(nonatomic, strong) NSTimer* timer;
@property(nonatomic) BOOL sleeping;
@property(nonatomic) BOOL adviceEnabled;
@property(nonatomic) NSUInteger frame;
@property(nonatomic) NSTimeInterval idleSince;
@property(nonatomic) NSTimeInterval nextAdvice;
@property(nonatomic) NSTimeInterval lastTick;
@property(nonatomic) NSTimeInterval nextAnimation;
@end

@implementation AppDelegate

- (void)applicationDidFinishLaunching:(NSNotification*)notification {
    NSMutableDictionary* images = [NSMutableDictionary new];
    for (NSString* path in [NSBundle.mainBundle pathsForResourcesOfType:@"png" inDirectory:nil]) {
        NSImage* image = [[NSImage alloc] initWithContentsOfFile:path];
        if (!image) {
            NSLog(@"Cannot load CatDog sprite: %@", path);
            [NSApp terminate:nil];
            return;
        }
        // Sprite dimensions are pixels; ignore the source PNG's print resolution.
        image.size = NSMakeSize(32, 32);
        images[path.lastPathComponent.stringByDeletingPathExtension] = image;
    }
    if (images.count != 22) {
        NSLog(@"CatDog requires all 22 bundled sprites. Build and run CatDog.app using make run.");
        [NSApp terminate:nil];
        return;
    }
    self.images = images;
    NSApp.applicationIconImage = images[@"still"];
    self.idleSince = NSProcessInfo.processInfo.systemUptime;
    self.lastTick = self.idleSince;
    self.nextAdvice = self.idleSince + 15;
    self.adviceEnabled = YES;

    NSMenu* menu = [NSMenu new];
    menu.delegate = self;
    [menu addItemWithTitle:@"About CatDog" action:@selector(showAbout:) keyEquivalent:@""];
    [menu addItem:[NSMenuItem separatorItem]];
    self.sleepItem = [menu addItemWithTitle:@"Put CatDog to Sleep" action:@selector(toggleSleep:) keyEquivalent:@""];
    self.adviceItem = [menu addItemWithTitle:@"Show Advice" action:@selector(toggleAdvice:) keyEquivalent:@""];
    [menu addItem:[NSMenuItem separatorItem]];
    [menu addItemWithTitle:@"Quit CatDog" action:@selector(terminate:) keyEquivalent:@"q"];
    for (NSMenuItem* item in menu.itemArray) {
        if (item.action != @selector(terminate:))
            item.target = self;
    }
    self.statusItem = [NSStatusBar.systemStatusBar statusItemWithLength:NSVariableStatusItemLength];
    NSImage* statusImage = [images[@"still"] copy];
    statusImage.size = NSMakeSize(22, 22);
    self.statusItem.button.image = statusImage;
    self.statusItem.button.toolTip = @"CatDog";
    self.statusItem.menu = menu;

    NSPoint mouse = NSEvent.mouseLocation;
    self.petWindow = CreatePanel(NSMakeRect(mouse.x - 100, mouse.y - 100, PetSize, PetSize));
    self.petView = [[CatDogView alloc] initWithFrame:NSMakeRect(0, 0, PetSize, PetSize)];
    self.petView.image = images[@"still"];
    self.petView.menu = menu;
    [self.petView setAccessibilityLabel:@"CatDog. Click to toggle advice; right-click for controls."];
    __weak AppDelegate* weakSelf = self;
    self.petView.onClick = ^{ [weakSelf toggleAdvice:nil]; };
    self.petWindow.contentView = self.petView;

    self.adviceWindow = CreatePanel(NSMakeRect(0, 0, 265, 74));
    self.bubble = [[SpeechBubble alloc] initWithFrame:self.adviceWindow.contentView.bounds];
    self.bubble.onDismiss = ^{ [weakSelf dismissAdvice]; };
    self.adviceWindow.contentView = self.bubble;
    [self.petWindow orderFrontRegardless];
    self.timer = [NSTimer timerWithTimeInterval:1.0 / 60
                                        repeats:YES
                                          block:^(__unused NSTimer* timer) { [weakSelf tick]; }];
    [NSRunLoop.mainRunLoop addTimer:self.timer forMode:NSRunLoopCommonModes];
    [NSNotificationCenter.defaultCenter addObserver:self
                                           selector:@selector(screensChanged:)
                                               name:NSApplicationDidChangeScreenParametersNotification
                                             object:nil];
    [self screensChanged:nil];
}

- (void)menuWillOpen:(NSMenu*)menu {
    self.sleepItem.title = self.sleeping ? @"Wake CatDog" : @"Put CatDog to Sleep";
    self.adviceItem.state = self.adviceEnabled ? NSControlStateValueOn : NSControlStateValueOff;
}

- (void)toggleSleep:(id)sender {
    [self dismissAdvice];
    self.sleeping = !self.sleeping;
    self.petView.image = self.images[self.sleeping ? @"sleep1" : @"still"];
    self.petView.needsDisplay = YES;
}

- (void)toggleAdvice:(id)sender {
    self.adviceEnabled = !self.adviceEnabled;
    [self dismissAdvice];
}

- (void)dismissAdvice {
    [self.adviceWindow orderOut:nil];
    self.idleSince = NSProcessInfo.processInfo.systemUptime;
    self.nextAdvice = self.idleSince + 15;
}

- (NSString*)specialAppearance {
    for (NSRunningApplication* app in NSWorkspace.sharedWorkspace.runningApplications) {
        NSString* name = app.localizedName.lowercaseString;
        if ([@[ @"pixel paint editor", @"pixel font editor", @"pixelmator pro", @"krita" ] containsObject:name])
            return @"artist";
        if ([@[ @"activity monitor", @"instruments" ] containsObject:name])
            return @"inspector";
    }
    return @"still";
}

- (void)tick {
    NSTimeInterval now = NSProcessInfo.processInfo.systemUptime;
    NSTimeInterval elapsed = Clamp(now - self.lastTick, 0, 1.0 / 15);
    self.lastTick = now;
    if (now >= self.nextAnimation) {
        self.frame = self.frame == 1 ? 2 : 1;
        self.nextAnimation = now + 0.25;
    }
    NSString* sprite;
    if (self.adviceWindow.visible) {
        return;
    } else if (self.sleeping) {
        sprite = [NSString stringWithFormat:@"sleep%lu", (unsigned long)self.frame];
        self.nextAdvice = now + 15;
    } else {
        NSPoint mouse = NSEvent.mouseLocation;
        NSRect rect = self.petWindow.frame;
        CGFloat dx = mouse.x < NSMinX(rect)   ? mouse.x - NSMinX(rect)
                     : mouse.x > NSMaxX(rect) ? mouse.x - NSMaxX(rect)
                                              : 0;
        CGFloat dy = mouse.y < NSMinY(rect)   ? mouse.y - NSMinY(rect)
                     : mouse.y > NSMaxY(rect) ? mouse.y - NSMaxY(rect)
                                              : 0;
        if (dx != 0 || dy != 0) {
            NSScreen* targetScreen = ScreenAtPoint(mouse);
            NSRect screen = targetScreen.visibleFrame;
            // Clamp the destination, not each step, so crossing a screen edge never snaps the window.
            dx = Clamp(rect.origin.x + dx, NSMinX(screen), NSMaxX(screen) - PetSize) - rect.origin.x;
            dy = Clamp(rect.origin.y + dy, NSMinY(screen), NSMaxY(screen) - PetSize) - rect.origin.y;
            NSScreen* currentScreen = ScreenAtPoint(NSMakePoint(NSMidX(rect), NSMidY(rect)));
            CGFloat speed = currentScreen == targetScreen ? 256 : 1200;
            CGFloat distance = hypot(dx, dy);
            CGFloat fraction = distance > 0 ? MIN(1, speed * elapsed / distance) : 0;
            NSPoint origin = NSMakePoint(rect.origin.x + dx * fraction, rect.origin.y + dy * fraction);
            if (!NSEqualPoints(origin, rect.origin)) {
                [self.petWindow setFrameOrigin:origin];
                self.idleSince = now;
                NSString* vertical = dy > 0 ? @"n" : dy < 0 ? @"s" : @"";
                NSString* horizontal = dx > 0 ? @"e" : dx < 0 ? @"w" : @"";
                sprite = [NSString stringWithFormat:@"%@%@run%lu", vertical, horizontal, (unsigned long)self.frame];
            }
        }
        if (!sprite) {
            self.sleeping = now - self.idleSince > 5;
            sprite = self.sleeping ? @"sleep1" : [self specialAppearance];
        }
        if (!self.sleeping && self.adviceEnabled && now >= self.nextAdvice) {
            [self showAdvice];
            sprite = [self specialAppearance];
            if ([sprite isEqualToString:@"still"])
                sprite = @"alert";
        }
    }
    self.petView.image = self.images[sprite];
    self.petView.needsDisplay = YES;
}

- (void)positionAdvice {
    NSRect pet = self.petWindow.frame;
    NSRect screen = ScreenAtPoint(NSMakePoint(NSMidX(pet), NSMidY(pet))).visibleFrame;
    NSSize size = self.adviceWindow.frame.size;
    CGFloat x = Clamp(NSMidX(pet) - size.width / 2, NSMinX(screen), NSMaxX(screen) - size.width);
    self.bubble.belowPet = NSMaxY(pet) + size.height > NSMaxY(screen);
    CGFloat y = self.bubble.belowPet ? NSMinY(pet) - size.height : NSMaxY(pet);
    self.bubble.tailX = Clamp(NSMidX(pet) - x, 8, size.width - 8);
    [self.adviceWindow setFrameOrigin:NSMakePoint(x, y)];
    self.bubble.needsDisplay = YES;
}

- (void)showAdvice {
    NSString* appearance = [self specialAppearance];
    NSArray<NSString*>* messages;
    if ([appearance isEqualToString:@"artist"]) {
        messages = @[
            @"It looks like you're creating art.\nWould you like some help?",
            @"It looks like you're making a meme.\nWould you like some help?"
        ];
    } else if ([appearance isEqualToString:@"inspector"]) {
        messages = @[
            @"It looks like you're profiling a program.\nWould you like some help?",
            @"It looks like you're interested in CPU\nusage. Would you like some help?"
        ];
    } else {
        messages = @[
            @"It looks like you're trying to debug\na program. Would you like some help?",
            @"It looks like you're trying to shave\na yak. Would you like some help?", @"Well Hello Friend!"
        ];
    }
    self.bubble.message = messages[arc4random_uniform((uint32_t)messages.count)];
    [self.bubble setAccessibilityLabel:self.bubble.message];
    [self positionAdvice];
    [self.adviceWindow orderFrontRegardless];
}

- (void)screensChanged:(NSNotification*)notification {
    NSRect rect = self.petWindow.frame;
    NSRect screen = ScreenAtPoint(NSMakePoint(NSMidX(rect), NSMidY(rect))).visibleFrame;
    [self.petWindow setFrameOrigin:NSMakePoint(Clamp(rect.origin.x, NSMinX(screen), NSMaxX(screen) - PetSize),
                                               Clamp(rect.origin.y, NSMinY(screen), NSMaxY(screen) - PetSize))];
    if (self.adviceWindow.visible)
        [self positionAdvice];
}

- (void)showAbout:(id)sender {
    [NSApp activateIgnoringOtherApps:YES];
    [NSApp orderFrontStandardAboutPanelWithOptions:@{
        NSAboutPanelOptionCredits : [[NSAttributedString alloc]
            initWithString:
                @"An AppKit clone of SerenityOS CatDog.\nSprites and original behavior by the SerenityOS developers.\n"
                @"Click CatDog to toggle advice. Click a speech bubble to dismiss it.\n"
                @"Right-click CatDog or use its menu bar icon to wake it or quit."],
    }];
}

- (void)applicationWillTerminate:(NSNotification*)notification {
    [self.timer invalidate];
    [NSNotificationCenter.defaultCenter removeObserver:self];
}

@end

int main(void) {
    @autoreleasepool {
        NSApplication* app = NSApplication.sharedApplication;
        AppDelegate* delegate = [AppDelegate new];
        app.delegate = delegate;
        [app run];
    }
    return EXIT_SUCCESS;
}
