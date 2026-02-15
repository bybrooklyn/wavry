<script>
  import Icon from "./Icon.svelte";
  let { icon, active, onclick } = $props();
</script>

<button {onclick} class="sidebar-icon" class:active>
  <Icon name={icon} size={18} />
</button>

<style>
  .sidebar-icon {
    width: 42px;
    height: 42px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-lg);
    border: 1px solid transparent;
    transition: background-color var(--animation-duration-standard)
        var(--animation-easing-standard),
      border-color var(--animation-duration-standard)
        var(--animation-easing-standard),
      color var(--animation-duration-standard) var(--animation-easing-standard);
    color: rgba(142, 142, 143, 0.95);
  }

  .sidebar-icon.active {
    background-color: rgba(0, 122, 255, 0.14);
    border-color: rgba(0, 122, 255, 0.35);
    color: var(--colors-accent-primary);
  }

  .sidebar-icon:hover:not(.active) {
    color: var(--colors-text-primary);
    background: rgba(255, 255, 255, 0.05);
  }
</style>
