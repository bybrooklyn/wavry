<script lang="ts">
    import { onMount } from "svelte";
    import { appState } from "$lib/appState.svelte";

    let { stream } = $props();
    let videoEl = $state<HTMLVideoElement | null>(null);
    let isPointerLocked = $state(false);
    let animationFrameId: number;

    $effect(() => {
        if (videoEl && stream) {
            videoEl.srcObject = stream;
        }
    });

    function requestPointerLock() {
        if (videoEl) {
            videoEl.requestPointerLock();
        }
    }

    function pollGamepads() {
        const gamepads = navigator.getGamepads();
        for (const gp of gamepads) {
            if (!gp) continue;
            
            // Map standard gamepad to RIFT format
            // Buttons: bitmask of first 16 buttons
            let buttons = 0;
            for (let i = 0; i < Math.min(gp.buttons.length, 16); i++) {
                if (gp.buttons[i].pressed) {
                    buttons |= (1 << i);
                }
            }

            // Axes: 4 standard axes (Left X/Y, Right X/Y) converted to i16
            const axes = [0, 0, 0, 0];
            for (let i = 0; i < Math.min(gp.axes.length, 4); i++) {
                // Scale -1.0..1.0 to -32767..32767
                // Y axes are often inverted in gamepads, but standard API says +1 is "down/right".
                // RIFT/Native usually expects the same.
                const val = Math.max(-1, Math.min(1, gp.axes[i]));
                axes[i] = Math.floor(val * 32767);
            }

            appState.sendInput(4, {
                gamepad_id: gp.index,
                buttons,
                axes
            });
        }
        animationFrameId = requestAnimationFrame(pollGamepads);
    }

    onMount(() => {
        animationFrameId = requestAnimationFrame(pollGamepads);

        const handleLockChange = () => {
            isPointerLocked = document.pointerLockElement === videoEl;
        };

        const handleMouseMove = (e: MouseEvent) => {
            if (!isPointerLocked) return;
            // Send relative movement only when locked
            appState.sendInput(1, { dx: e.movementX, dy: e.movementY });
        };

        const handleMouseDown = (e: MouseEvent) => {
            if (!isPointerLocked && videoEl) {
                requestPointerLock();
                return;
            }
            appState.sendControl({
                type: "control",
                control: {
                    type: "mouse_button",
                    button: e.button,
                    pressed: true,
                    timestamp_us: Math.floor(performance.now() * 1000)
                }
            });
        };

        const handleMouseUp = (e: MouseEvent) => {
            if (!isPointerLocked) return;
            appState.sendControl({
                type: "control",
                control: {
                    type: "mouse_button",
                    button: e.button,
                    pressed: false,
                    timestamp_us: Math.floor(performance.now() * 1000)
                }
            });
        };

        const handleKeyDown = (e: KeyboardEvent) => {
            if (!isPointerLocked) return;
            // Prevent default browser actions (like F5 or Ctrl+W) if possible/desired
            // but usually we just want to block game keys
            appState.sendControl({
                type: "control",
                control: {
                    type: "key",
                    keycode: e.keyCode,
                    pressed: true,
                    timestamp_us: Math.floor(performance.now() * 1000)
                }
            });
        };

        const handleKeyUp = (e: KeyboardEvent) => {
            if (!isPointerLocked) return;
            appState.sendControl({
                type: "control",
                control: {
                    type: "key",
                    keycode: e.keyCode,
                    pressed: false,
                    timestamp_us: Math.floor(performance.now() * 1000)
                }
            });
        };

        document.addEventListener("pointerlockchange", handleLockChange);
        // Use document listeners for mouse/key when locked to capture everything
        document.addEventListener("mousemove", handleMouseMove);
        document.addEventListener("mousedown", handleMouseDown);
        document.addEventListener("mouseup", handleMouseUp);
        window.addEventListener("keydown", handleKeyDown);
        window.addEventListener("keyup", handleKeyUp);

        return () => {
            cancelAnimationFrame(animationFrameId);
            document.removeEventListener("pointerlockchange", handleLockChange);
            document.removeEventListener("mousemove", handleMouseMove);
            document.removeEventListener("mousedown", handleMouseDown);
            document.removeEventListener("mouseup", handleMouseUp);
            window.removeEventListener("keydown", handleKeyDown);
            window.removeEventListener("keyup", handleKeyUp);
        };
    });
</script>

<div class="video-container">
    {#if stream}
        <!-- svelte-ignore a11y_media_has_caption -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <video
            bind:this={videoEl}
            autoplay
            playsinline
            controls={false}
            onclick={requestPointerLock}
        ></video>
    {:else}
        <div class="placeholder">
            <span class="spinner"></span>
            <p>Waiting for video stream...</p>
        </div>
    {/if}
</div>

<style>
    .video-container {
        width: 100%;
        height: 100%;
        background: #000;
        position: relative;
        display: flex;
        align-items: center;
        justify-content: center;
        overflow: hidden;
        border-radius: var(--radius-lg);
    }

    video {
        width: 100%;
        height: 100%;
        object-fit: contain;
    }

    .placeholder {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: var(--spacing-md);
        color: var(--colors-text-secondary);
    }

    .spinner {
        width: 40px;
        height: 40px;
        border: 4px solid rgba(255, 255, 255, 0.1);
        border-top-color: var(--colors-accent-primary);
        border-radius: 50%;
        animation: spin 1s linear infinite;
    }

    @keyframes spin {
        to { transform: rotate(360deg); }
    }
</style>
