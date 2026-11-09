<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Picks" });

const rows = ref(["alpha", "beta"]);
const picked = ref("");
</script>

<template>
  <ul>
    <li v-for="r in rows" :key="r">
      <button @click="picked = r">{{ r }}</button>
    </li>
  </ul>
</template>
