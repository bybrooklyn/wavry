<script lang="ts">
    import { onMount } from "svelte";

    let { name, size = 24, class: className = "" } = $props();

    let svgContent = $state("");
    let loadError = $state(false);

    // Inline placeholder SVG (no import needed)
    const placeholderSvg = `<svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
    <rect x="6" y="6" width="12" height="12" stroke="currentColor" stroke-width="2" fill="none" stroke-dasharray="2,2"/>
    <text x="12" y="13" font-family="monospace" font-size="8" fill="currentColor" text-anchor="middle">?</text>
  </svg>`;

    onMount(async () => {
        try {
            // Dynamically import the SVG
            // In production build, Vite will bundle these as assets
            const module = await import(`../assets/icons/${name}.svg?raw`);
            svgContent = module.default;
        } catch (err) {
            console.warn(`Icon "${name}" not found, using placeholder`);
            loadError = true;
            svgContent = placeholderSvg;
        }
    });
</script>

{#if svgContent}
    <div
        class="wavry-icon {className}"
        style="width: {size}px; height: {size}px; display: inline-flex; align-items: center; justify-content: center;"
        class:placeholder={loadError}
    >
        {@html svgContent}
    </div>
{:else}
    <div style="width: {size}px; height: {size}px;" class="icon-loading"></div>
{/if}

<style>
    .wavry-icon :global(svg) {
        width: 100%;
        height: 100%;
        color: inherit;
    }

    .wavry-icon.placeholder :global(svg) {
        opacity: 0.3;
    }

    .icon-loading {
        background: rgba(255, 255, 255, 0.1);
        border-radius: 4px;
    }
</style>
