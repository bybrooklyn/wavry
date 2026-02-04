<script>
  import "../app.css";
  let { children } = $props();
</script>

{@render children()}

<style>
  :global(:root) {
    /* Colors */
    --colors-bg-base: #212121;
    --colors-bg-sidebar: #1A1A1A;
    --colors-bg-elevation1: #262626;
    --colors-bg-elevation2: #2E2E2E;
    --colors-bg-elevation3: #333333;
    --colors-bg-modal: rgba(26, 26, 26, 0.5);
    --colors-bg-rowHover: rgba(255, 255, 255, 0.02);
    
    --colors-accent-primary: #007AFF;
    --colors-accent-success: #34C759;
    --colors-accent-danger: #FF3B30;
    
    --colors-text-primary: #FFFFFF;
    --colors-text-secondary: #8E8E8F;
    
    --colors-border-subtle: rgba(255, 255, 255, 0.05);
    --colors-border-input: rgba(255, 255, 255, 0.1);

    /* Spacing */
    --spacing-xs: 4px;
    --spacing-sm: 8px;
    --spacing-md: 12px;
    --spacing-lg: 16px;
    --spacing-xl: 20px;
    --spacing-xxl: 32px;
    --spacing-xxxl: 40px;

    /* Radius */
    --radius-sm: 4px;
    --radius-md: 6px;
    --radius-lg: 8px;
    --radius-xl: 12px;
    --radius-xxl: 24px;

    /* Typography */
    --font-size-titleLg: 36px;
    --font-size-titleMd: 32px;
    --font-size-input: 18px;
    --font-size-body: 13px;
    --font-size-caption: 10px;

    --font-weight-light: 300;
    --font-weight-regular: 400;
    --font-weight-semibold: 600;
    --font-weight-bold: 700;

    /* Animations */
    --animation-duration-standard: 0.35s;
    --animation-easing-standard: cubic-bezier(0.42, 0, 0.58, 1);
  }

  :global(body) {
    margin: 0;
    padding: 0;
    background-color: var(--colors-bg-base);
    color: var(--colors-text-primary);
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif, "Apple Color Emoji", "Segoe UI Emoji";
    -webkit-font-smoothing: antialiased;
    overflow: hidden; /* Desktop app feel */
    height: 100vh;
    width: 100vw;
    color-scheme: dark;
  }
</style>
