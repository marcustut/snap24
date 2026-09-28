// Adopt the UIKit scene-based lifecycle on iOS 27.
//
// winit 0.30 creates its window before any scene exists and never attaches it to
// a UIWindowScene. Apps linked against the iOS 27 SDK that don't adopt scenes
// are killed at launch by UIKit — the crash log names
// `UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption`.
//
// Fix: swizzle -[UIWindow makeKeyAndVisible]. When winit's orphan window goes
// key, attach it to the connected scene; if the scene hasn't connected yet, park
// it and flush on connect. winit's event loop is driven by app-level
// NSNotifications, which still fire under the scene lifecycle, so no lifecycle
// callbacks need forwarding.
//
// Ported from bevy_ios_toolkit's `Sources/Platform/Scene.swift` (MIT).

#import <UIKit/UIKit.h>
#import <objc/runtime.h>

static UIWindowScene *S24ConnectedScene = nil;
static NSHashTable<UIWindow *> *S24Pending = nil;
static BOOL S24Installed = NO;

@interface S24SceneDelegate : UIResponder <UIWindowSceneDelegate>
@end

@interface UIWindow (Snap24Scene)
- (void)s24_makeKeyAndVisible;
@end

@implementation UIWindow (Snap24Scene)

- (void)s24_makeKeyAndVisible {
    UIWindowScene *scene = self.windowScene;
    BOOL connected = scene != nil &&
        [[[UIApplication sharedApplication] connectedScenes] containsObject:scene];

    // This workaround owns only winit's orphan; every other window behaves as before.
    if (!connected && [self isKindOfClass:NSClassFromString(@"WinitUIWindow")]) {
        if (S24ConnectedScene != nil) {
            self.windowScene = S24ConnectedScene;
            // `effectiveGeometry` is iOS 26+, but this app deploys to iOS 15.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
            self.frame = S24ConnectedScene.coordinateSpace.bounds;
#pragma clang diagnostic pop
        } else {
            // didFinishLaunching can show the window before willConnect, and
            // UIApplication.windows cannot find the orphan afterwards.
            if (S24Pending == nil) {
                S24Pending = [NSHashTable weakObjectsHashTable];
            }
            [S24Pending addObject:self];
        }
    }

    // After the exchange this selector invokes UIKit's original method.
    [self s24_makeKeyAndVisible];
}

@end

@implementation S24SceneDelegate

- (void)scene:(UIScene *)scene
    willConnectToSession:(UISceneSession *)session
                 options:(UISceneConnectionOptions *)options {
    if (![scene isKindOfClass:[UIWindowScene class]]) {
        return;
    }
    if (![session.role isEqualToString:UIWindowSceneSessionRoleApplication]) {
        return;
    }

    S24ConnectedScene = (UIWindowScene *)scene;

    NSArray<UIWindow *> *pending = [S24Pending allObjects];
    [S24Pending removeAllObjects];
    for (UIWindow *window in pending) {
        // Safe after `removeAllObjects`: `pending` holds strong references.
        [window makeKeyAndVisible];
    }
}

// UIKit already posts the app-level notifications winit observes, so scene
// callbacks deliberately do not re-post them (that would duplicate events).

@end

static void S24Install(void) {
    if (S24Installed) {
        return;
    }
    Method original = class_getInstanceMethod([UIWindow class], @selector(makeKeyAndVisible));
    Method replacement = class_getInstanceMethod([UIWindow class], @selector(s24_makeKeyAndVisible));
    if (original == NULL || replacement == NULL) {
        return;
    }
    method_exchangeImplementations(original, replacement);
    S24Installed = YES;
}

/// Called from Rust before the event loop runs, so the swizzle is in place
/// before winit makes its window visible. Referencing the class also keeps it
/// (and this translation unit) in the binary, since Info.plist names it only at
/// runtime.
void s24_register_scene_delegate(void) {
    (void)[S24SceneDelegate class];
    S24Install();
}
