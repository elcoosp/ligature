<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Names" });

const names = ref(["alpha", "beta", "gamma"]);
</script>

<template>
  <ul>
    <li v-for="n in names">{{ n }}</li>
  </ul>
</template>
