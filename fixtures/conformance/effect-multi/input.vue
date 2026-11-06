<script setup lang="ts">
import { ref, watchEffect } from "vue";

defineOptions({ name: "Sync" });

const tick = ref(0);
const log = ref("");
const count = ref(0);
watchEffect(() => {
  log.value = tick.value.toString();
  count.value = tick.value * 2;
});
</script>

<template>
  <em>{{ log }}</em>
</template>
