<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Tags" });

const tags = ref(["alpha", "beta", "gamma"]);
</script>

<template>
  <ul>
    <li v-for="t in tags" :key="t">{{ t }}</li>
  </ul>
</template>
