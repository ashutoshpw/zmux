#import <AuthenticationServices/AuthenticationServices.h>
#import <objc/runtime.h>
#import <pthread.h>
#import "cloud-auth-callback.h"

// Use the actual SDK declaration: a Swift replacement changes closure isolation.
static pthread_mutex_t completion_lock = PTHREAD_MUTEX_INITIALIZER;
static ASWebAuthenticationSessionCompletionHandler captured_completion;
static bool installed;

@interface ASWebAuthenticationSession (ZmuxAuthenticationCallbackFixture)
- (instancetype)zmux_fixture_initWithURL:(NSURL *)url
                     callbackURLScheme:(NSString *)scheme
                     completionHandler:(ASWebAuthenticationSessionCompletionHandler)completion
    NS_METHOD_FAMILY(init);
- (BOOL)zmux_fixture_start;
@end

@implementation ASWebAuthenticationSession (ZmuxAuthenticationCallbackFixture)
- (instancetype)zmux_fixture_initWithURL:(NSURL *)url
                     callbackURLScheme:(NSString *)scheme
                     completionHandler:(ASWebAuthenticationSessionCompletionHandler)completion {
    pthread_mutex_lock(&completion_lock);
    captured_completion = [completion copy];
    pthread_mutex_unlock(&completion_lock);
    // After the exchange this selector calls the SDK's original initializer.
    return [self zmux_fixture_initWithURL:url callbackURLScheme:scheme completionHandler:completion];
}

- (BOOL)zmux_fixture_start {
    // Never launch Safari or start an authentication request in this fixture.
    return YES;
}
@end

bool zmux_auth_callback_install(void) {
    if (installed) return false;
    Class type = [ASWebAuthenticationSession class];
    Method initialize = class_getInstanceMethod(type, @selector(initWithURL:callbackURLScheme:completionHandler:));
    Method capture = class_getInstanceMethod(type, @selector(zmux_fixture_initWithURL:callbackURLScheme:completionHandler:));
    Method start = class_getInstanceMethod(type, @selector(start));
    Method suppress = class_getInstanceMethod(type, @selector(zmux_fixture_start));
    if (!initialize || !capture || !start || !suppress) return false;
    method_exchangeImplementations(initialize, capture);
    method_exchangeImplementations(start, suppress);
    installed = true;
    return true;
}

bool zmux_auth_callback_has_completion(void) {
    pthread_mutex_lock(&completion_lock);
    bool present = captured_completion != nil;
    pthread_mutex_unlock(&completion_lock);
    return present;
}

bool zmux_auth_callback_invoke_failure(void) {
    pthread_mutex_lock(&completion_lock);
    ASWebAuthenticationSessionCompletionHandler completion = captured_completion;
    pthread_mutex_unlock(&completion_lock);
    if (!completion) return false;
    NSError *failure = [NSError errorWithDomain:@"ZmuxAuthenticationFixture" code:7
                                      userInfo:@{NSLocalizedDescriptionKey: @"SDK dry-run refused (fixture)."}];
    completion(nil, failure);
    return true;
}

void zmux_auth_callback_reset(void) {
    if (installed) {
        Class type = [ASWebAuthenticationSession class];
        method_exchangeImplementations(class_getInstanceMethod(type, @selector(initWithURL:callbackURLScheme:completionHandler:)),
                                       class_getInstanceMethod(type, @selector(zmux_fixture_initWithURL:callbackURLScheme:completionHandler:)));
        method_exchangeImplementations(class_getInstanceMethod(type, @selector(start)),
                                       class_getInstanceMethod(type, @selector(zmux_fixture_start)));
        installed = false;
    }
    pthread_mutex_lock(&completion_lock);
    captured_completion = nil;
    pthread_mutex_unlock(&completion_lock);
}
