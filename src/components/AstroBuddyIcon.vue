<template>
  <svg class="buddy-icon" viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <defs>
      <!-- 面罩实心、双眼靠蒙版挖空：单色图标下也能一眼看出「大面罩 + 两只眼」 -->
      <mask id="buddyIconVisor" maskUnits="userSpaceOnUse" x="0" y="0" width="24" height="24">
        <rect width="24" height="24" fill="#000" />
        <ellipse cx="12" cy="9.3" rx="5.6" ry="4.9" fill="#fff" />
        <circle cx="10.3" cy="9.2" r="1.1" fill="#000" />
        <circle cx="13.7" cy="9.2" r="1.1" fill="#000" />
      </mask>
    </defs>

    <!-- 耳罩 -->
    <ellipse cx="4.7" cy="9.6" rx="1.1" ry="1.8" fill="currentColor" opacity="0.45" />
    <ellipse cx="19.3" cy="9.6" rx="1.1" ry="1.8" fill="currentColor" opacity="0.45" />
    <!-- 躯干（比颈部宽，避免看成灯泡） -->
    <rect x="5.9" y="15.6" width="12.2" height="5.8" rx="2.8" stroke="currentColor" stroke-width="1.5" />
    <!-- 头盔 -->
    <ellipse cx="12" cy="9.2" rx="7.1" ry="6.7" stroke="currentColor" stroke-width="1.6" />
    <!-- 面罩 + 双眼 -->
    <rect x="0" y="0" width="24" height="24" fill="currentColor" mask="url(#buddyIconVisor)" />
    <!-- 胸口徽标 -->
    <circle cx="12" cy="18.5" r="1.15" fill="currentColor" opacity="0.8" />
  </svg>
</template>

<style scoped>
.buddy-icon {
  width: 1em;
  height: 1em;
  display: inline-block;
  vertical-align: -0.125em;
}
</style>
