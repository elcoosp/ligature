<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Rows" });

const rows = ref(["alpha", "beta"]);
</script>

<template>
  <ul>
    <li v-for="(r, i) in rows">{{ i }} {{ r }}</li>
  </ul>
</template>
