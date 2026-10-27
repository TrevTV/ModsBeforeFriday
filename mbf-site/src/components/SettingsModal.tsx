import { useState } from "react";
import { Modal } from "./Modal";
import { Slider } from "./Slider";

export function SettingsModal({ isVisible, onClose }: { isVisible: boolean, onClose: () => void }) {
    return <Modal isVisible={isVisible}>
        <h2>Settings</h2>

        <div className="confirmButtons">
            <button onClick={onClose}>OK</button>
        </div>
    </Modal>
}