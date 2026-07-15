// macOS pairing-beacon advertiser (new-device role) — docs/pairing-v2.md, docs/contact-system.md.
//
// CoreBluetooth's CBPeripheralManager needs a delegate (to learn when Bluetooth powers on) and a run loop
// to drive it — both far cleaner in Objective-C than raw objc2, so the whole advertiser is this shim and
// Rust just calls the two C entry points below. All CoreBluetooth work is dispatched to the MAIN queue
// (queue:nil delegate), which the app's NSApp event loop already runs, so there's no thread/run-loop of
// our own to manage. The beacon is a single 128-bit service UUID — the ONLY payload Apple lets an app
// advertise — matching every other courier (fgtw::pair::beacon_id, 16 bytes, byte 0 = most-significant).

#import <CoreBluetooth/CoreBluetooth.h>
#import <Foundation/Foundation.h>

@interface PhotonAdv : NSObject <CBPeripheralManagerDelegate>
@property (nonatomic, strong) CBPeripheralManager *mgr;
@property (nonatomic, strong) NSData *pendingUuid; // nil = not advertising; else the 16-byte beacon id
@end

@implementation PhotonAdv

// Advertise the pending beacon iff Bluetooth is powered on; a no-op otherwise (the delegate re-runs this the moment it powers on).
- (void)advertiseNow {
    if (!self.pendingUuid || self.mgr.state != CBManagerStatePoweredOn) {
        return;
    }
    CBUUID *u = [CBUUID UUIDWithData:self.pendingUuid];
    [self.mgr startAdvertising:@{ CBAdvertisementDataServiceUUIDsKey: @[u] }];
}

- (void)peripheralManagerDidUpdateState:(CBPeripheralManager *)peripheral {
    // Fires on power-on (and every state change); advertise once we're allowed to.
    [self advertiseNow];
}

@end

static PhotonAdv *g_adv = nil;

// Start (or replace) the advertised beacon. `bytes` is the 16-byte service UUID; safe to call from any thread.
void photon_ble_adv_start(const uint8_t *bytes, size_t len) {
    NSData *uuid = [NSData dataWithBytes:bytes length:len];
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!g_adv) {
            g_adv = [PhotonAdv new];
        }
        g_adv.pendingUuid = uuid;
        if (!g_adv.mgr) {
            // First use: creating the manager triggers the permission prompt + a state callback that advertises.
            g_adv.mgr = [[CBPeripheralManager alloc] initWithDelegate:g_adv queue:nil options:nil];
        } else {
            [g_adv advertiseNow];
        }
    });
}

// Stop advertising (ceremony ended). Safe from any thread; keeps the manager alive for a fast restart.
void photon_ble_adv_stop(void) {
    dispatch_async(dispatch_get_main_queue(), ^{
        if (g_adv) {
            g_adv.pendingUuid = nil;
            [g_adv.mgr stopAdvertising];
        }
    });
}
