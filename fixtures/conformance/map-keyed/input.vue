<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Rows" });

const items = ref(["alpha", "beta"]);
</script>

<template>
  <ul>
    <li v-for="item in items" :key="item">{{ item }}</li>
  </ul>
</template>
