<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "List" });

const items = ref(["alpha", "beta"]);
</script>

<template>
  <ul>
    <li v-for="item in items">{{ item }}</li>
  </ul>
</template>
