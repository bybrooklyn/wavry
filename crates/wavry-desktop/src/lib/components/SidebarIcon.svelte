<script>
  import Icon from "./Icon.svelte";
  let { icon, active, onclick } = $props();
</script>

<button {onclick} class="sidebar-icon" class:active>
  <Icon name={icon} size={20} />
</button>

<style>
  .sidebar-icon {
    width: 40px;
    height: 40px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-lg);
    transition: background-color var(--animation-duration-standard)
      var(--animation-easing-standard);
    color: var(--colors-text-secondary);
  }

  .sidebar-icon.active {
    background-color: rgba(0, 122, 255, 0.1);
    color: var(--colors-accent-primary);
  }
</style>
