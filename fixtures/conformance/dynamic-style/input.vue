<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Swatch" });

const color = ref("crimson");
</script>

<template>
  <div :style="{ color: color, 'font-size': '14px' }">swatch</div>
</template>
