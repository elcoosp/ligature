<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "PickPanel" });

const on = ref(true);
const rows = ref(["alpha", "beta"]);
const picked = ref("");
</script>

<template>
  <ul v-if="on">
    <li v-for="r in rows" :key="r">
      <button @click="picked = r">{{ r }}</button>
    </li>
  </ul>
</template>
