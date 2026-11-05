<script setup lang="ts">
import { ref, watchEffect } from "vue";

defineOptions({ name: "Watcher" });

const tick = ref(0);
const log = ref("");
watchEffect(() => (log.value = tick.value.toString()));
</script>

<template>
  <em>{{ log }}</em>
</template>
