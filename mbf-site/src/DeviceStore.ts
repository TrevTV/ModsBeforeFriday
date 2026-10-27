import { Adb } from '@yume-chan/adb';
import { create } from "zustand";
/**
 * Device store is used to save the data about the device and a reference to the ADB connection.
 */

export interface DeviceStore {
    /**
     * The ADB connection to the device.
     */
    device: Adb | null;
    /**
     * The Android version of the device, if available.
     */
    androidVersion: number | null;
    setAndroidVersion: (version: number | null) => void;
    setDevice: (adb: Adb | null) => void;

}

export const useDeviceStore = create<DeviceStore>(set => ({
    device: null,
    androidVersion: null,
    setAndroidVersion: (version: number | null) => set(() => ({ androidVersion: version })),
    setDevice: (device: Adb | null) => set(() => ({ device: device }))
}));
