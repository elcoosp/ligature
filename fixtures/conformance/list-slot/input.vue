<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "App" });

const rows = ref(["alpha", "beta"]);
</script>

<template>
  <main><slot v-for="t in rows" :key="t" name="row"><i class="row">{{ t }}</i></slot></main>
</template>
