<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Form" });

const name = ref("");
</script>

<template>
  <input :value="name" @input="name = 'typed'" @click="name = name.trim()" />
</template>
