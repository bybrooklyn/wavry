<script lang="ts">
    import { appState, type DeltaConfig } from "$lib/appState.svelte";

    let stats = $derived({
        bitrate: (appState.ccBitrate / 1000).toFixed(1) + " Mbps",
        state: appState.ccState,
    });

    const categories: {
        name: string;
        key: keyof DeltaConfig;
        min: number;
        max: number;
        step: number;
        label: string;
    }[] = [
        {
            name: "Latency",
            key: "target_delay_us",
            min: 5000,
            max: 100000,
            step: 1000,
            label: "Target Delay (μs)",
        },
        {
            name: "Throughput",
            key: "increase_kbps",
            min: 100,
            max: 5000,
            step: 100,
            label: "Add. Increase (kbps)",
        },
        {
            name: "Recovery",
            key: "beta",
            min: 0.5,
            max: 0.95,
            step: 0.01,
            label: "Back-off Factor (β)",
        },
        {
            name: "Sensitivity",
            key: "epsilon_us",
            min: 10,
            max: 1000,
            step: 10,
            label: "Slope Floor (ε)",
        },
    ];
</script>

<div class="delta-tuning p-6 space-y-8">
    <div
        class="header flex justify-between items-center bg-zinc-900/50 p-6 rounded-2xl border border-white/5 backdrop-blur-xl"
    >
        <div class="info">
            <h2
                class="text-2xl font-bold bg-gradient-to-r from-blue-400 to-indigo-400 bg-clip-text text-transparent"
            >
                DELTA Congestion Control
            </h2>
            <p class="text-zinc-500 text-sm mt-1">
                Real-time network performance monitoring & tuning
            </p>
        </div>
        <div class="metrics flex gap-6">
            <div
                class="metric bg-black/40 px-6 py-3 rounded-xl border border-white/5"
            >
                <span
                    class="block text-[10px] uppercase tracking-widest text-zinc-500 font-bold mb-1"
                    >State</span
                >
                <span
                    class="text-xl font-mono {appState.ccState === 'Congested'
                        ? 'text-red-400'
                        : appState.ccState === 'Rising'
                          ? 'text-yellow-400'
                          : 'text-emerald-400'}"
                >
                    {appState.ccState.toUpperCase()}
                </span>
            </div>
            <div
                class="metric bg-black/40 px-6 py-3 rounded-xl border border-white/5"
            >
                <span
                    class="block text-[10px] uppercase tracking-widest text-zinc-500 font-bold mb-1"
                    >Bitrate</span
                >
                <span class="text-xl font-mono text-zinc-100"
                    >{stats.bitrate}</span
                >
            </div>
        </div>
    </div>

    <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        {#each categories as cat}
            <div
                class="card bg-zinc-900/40 p-6 rounded-2xl border border-white/5 hover:border-white/10 transition-colors"
            >
                <div class="flex justify-between items-center mb-4">
                    <label
                        for={cat.key}
                        class="text-sm font-semibold text-zinc-300"
                        >{cat.label}</label
                    >
                    <span
                        class="text-xs font-mono text-zinc-500 bg-black/30 px-2 py-1 rounded"
                        >{appState.ccConfig[cat.key]}</span
                    >
                </div>
                <input
                    id={cat.key}
                    type="range"
                    min={cat.min}
                    max={cat.max}
                    step={cat.step}
                    bind:value={appState.ccConfig[cat.key]}
                    onchange={() => appState.updateCCConfig()}
                    class="w-full h-1.5 bg-zinc-800 rounded-lg appearance-none cursor-pointer accent-blue-500"
                />
                <div
                    class="flex justify-between mt-2 text-[10px] text-zinc-600 font-bold uppercase tracking-tighter"
                >
                    <span>Min</span>
                    <span>Max</span>
                </div>
            </div>
        {/each}
    </div>

    <div
        class="persistence-info bg-blue-500/5 p-4 rounded-xl border border-blue-500/10 flex items-start gap-3"
    >
        <div class="icon mt-0.5 text-blue-400">
            <svg
                xmlns="http://www.w3.org/2000/svg"
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                ><circle cx="12" cy="12" r="10" /><line
                    x1="12"
                    y1="16"
                    x2="12"
                    y2="12"
                /><line x1="12" y1="8" x2="12.01" y2="8" /></svg
            >
        </div>
        <p class="text-xs text-blue-300/80 leading-relaxed">
            These parameters are applied instantly to the active host loop.
            Tuning allows optimizing for specific network types (e.g., Starlink,
            WiFi 6, fiber).
            <span class="font-bold">Target Delay</span> is the most critical factor
            for visual stability.
        </p>
    </div>
</div>

<style>
    input[type="range"]::-webkit-slider-thumb {
        -webkit-appearance: none;
        appearance: none;
        width: 14px;
        height: 14px;
        background: #3b82f6;
        cursor: pointer;
        border-radius: 50%;
        border: 2px solid #18181b;
        box-shadow: 0 0 0 0 rgba(59, 130, 246, 0.5);
        transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
    }

    input[type="range"]:active::-webkit-slider-thumb {
        transform: scale(1.2);
        box-shadow: 0 0 0 8px rgba(59, 130, 246, 0.1);
    }
</style>
