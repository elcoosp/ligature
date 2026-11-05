<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Gate" });

const open = ref(true);
</script>

<template>
  <p v-if="open">Open</p>
  <p v-else>Closed</p>
</template>
